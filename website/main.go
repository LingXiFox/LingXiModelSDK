package main

import (
 "bytes"
 "context"
 "embed"
 "errors"
 "flag"
 "fmt"
 "html/template"
 "io/fs"
 "log"
 "net/http"
 "net/url"
 "os"
 "os/signal"
 "strconv"
 "strings"
 "syscall"
 "time"
)

//go:embed templates/*.html static/*
var assets embed.FS
var pages=template.Must(template.New("index.html").Funcs(template.FuncMap{
 "safeURL":func(s string)string{u,e:=url.Parse(s);if e!=nil||u.Host==""||(u.Scheme!="https"&&u.Scheme!="http"){return ""};return s},
 "short":func(s string)string{s=strings.TrimPrefix(s,"sha256:");if len(s)>12{return s[:12]};return s},
}).ParseFS(assets,"templates/*.html"))

type Filter struct{ ID,Label string }
var filters=[]Filter{{"all","全部"},{"reasoning","深度思考"},{"vision","视觉输入"},{"tools","工具调用"},{"structured","结构化输出"},{"largeContext","≥128k 窗口"}}
type View struct{
 Catalog Catalog
 Providers []Provider
 Models []Model
 Selected *Provider
 Q,PID,Cap string
 Filters []Filter
 Total,Start,End,Page int
 Previous,Next string
 Unavailable bool
}
func (v View) Link(pid,cap string,page int)string{
 q:=url.Values{};if pid!=""{q.Set("provider",pid)};if cap!=""&&cap!="all"{q.Set("cap",cap)};if v.Q!=""{q.Set("q",v.Q)};if page>1{q.Set("page",strconv.Itoa(page))};if len(q)==0{return "/"};return "/?"+q.Encode()
}
func matches(m Model,v View)bool{
 if v.PID!=""&&v.PID!=m.ProviderID{return false}
 switch v.Cap{case "reasoning":if !m.Reasoning{return false};case "vision":if !m.Vision{return false};case "tools":if !m.Tools{return false};case "structured":if !m.Structured{return false};case "largeContext":if m.ContextN<128000{return false}}
 return v.Q==""||strings.Contains(strings.ToLower(strings.Join([]string{m.ID,m.Name,m.Family,m.ProviderID,m.ProviderName}," ")),strings.ToLower(v.Q))
}
func view(s *Snapshot,q url.Values)View{
 v:=View{Q:strings.TrimSpace(q.Get("q")),PID:q.Get("provider"),Cap:q.Get("cap"),Filters:filters,Page:1}
 if len(v.Q)>512{v.Q=v.Q[:512]}
 valid:=false;for _,f:=range filters{if v.Cap==f.ID{valid=true}};if !valid{v.Cap="all"}
 if s==nil{v.Unavailable=true;return v}
 v.Catalog=s.Catalog;v.Providers=s.Providers
 for i:=range s.Providers{if s.Providers[i].ID==v.PID{v.Selected=&s.Providers[i];break}}
 if v.Selected==nil{v.PID=""}
 const size=60
 filtered:=[]Model{};for _,m:=range s.Models{if matches(m,v){filtered=append(filtered,m)}}
 v.Total=len(filtered);n,e:=strconv.Atoi(q.Get("page"));if e==nil&&n>0{v.Page=n};max:=(v.Total+size-1)/size;if max<1{max=1};if v.Page>max{v.Page=max}
 start:=(v.Page-1)*size;end:=start+size;if end>v.Total{end=v.Total};v.Models=filtered[start:end];v.End=end;if end>0{v.Start=start+1}
 if v.Page>1{v.Previous=v.Link(v.PID,v.Cap,v.Page-1)};if v.Page<max{v.Next=v.Link(v.PID,v.Cap,v.Page+1)}
 return v
}
func publicJSON(w http.ResponseWriter,r *http.Request,b []byte,etag,cache string){
 w.Header().Set("Content-Type","application/json; charset=utf-8");w.Header().Set("ETag",etag);w.Header().Set("Cache-Control",cache)
 w.Header().Set("Access-Control-Allow-Origin","*");w.Header().Set("Access-Control-Allow-Methods","GET, HEAD, OPTIONS");w.Header().Set("Access-Control-Allow-Headers","Content-Type, If-None-Match, If-Modified-Since")
 w.Header().Set("Access-Control-Expose-Headers","ETag")
 if r.Method==http.MethodOptions{w.WriteHeader(http.StatusNoContent);return}
 http.ServeContent(w,r,"data.json",time.Time{},bytes.NewReader(b))
}
func handler(s *Store)http.Handler{
 mux:=http.NewServeMux()
 static,_:=fs.Sub(assets,"static");mux.Handle("GET /static/",http.StripPrefix("/static/",http.FileServer(http.FS(static))))
 mux.HandleFunc("GET /index.html",func(w http.ResponseWriter,r *http.Request){http.Redirect(w,r,"/",http.StatusMovedPermanently)})
 for _,path:=range []string{"/models.json","/publication.json"}{
  p:=path
  h:=func(w http.ResponseWriter,r *http.Request){
   snap:=s.current.Load();if snap==nil{w.Header().Set("Cache-Control","no-store");http.Error(w,"catalog unavailable",503);return}
   if p=="/models.json"{publicJSON(w,r,snap.Raw,snap.ETag,"public, max-age=1800, stale-while-revalidate=86400")}else{publicJSON(w,r,snap.Publication,snap.PublicationETag,"public, max-age=60, must-revalidate")}
  }
  mux.HandleFunc("GET "+p,h);mux.HandleFunc("OPTIONS "+p,h)
 }
 mux.HandleFunc("GET /healthz",func(w http.ResponseWriter,r *http.Request){w.Header().Set("Cache-Control","no-store");if s.current.Load()==nil{http.Error(w,"no catalog",503);return};fmt.Fprintln(w,"ok")})
 mux.HandleFunc("GET /{$}",func(w http.ResponseWriter,r *http.Request){
  v:=view(s.current.Load(),r.URL.Query());var b bytes.Buffer
  if err:=pages.ExecuteTemplate(&b,"index.html",v);err!=nil{log.Printf("template: %v",err);http.Error(w,"internal error",500);return}
  w.Header().Set("Content-Type","text/html; charset=utf-8");w.Header().Set("Cache-Control","public, max-age=60, must-revalidate")
  if v.Unavailable{w.Header().Set("Cache-Control","no-store");w.WriteHeader(503)}
  if r.Method!="HEAD"{_,_=w.Write(b.Bytes())}
 })
 return http.HandlerFunc(func(w http.ResponseWriter,r *http.Request){
  w.Header().Set("X-Content-Type-Options","nosniff");w.Header().Set("Referrer-Policy","strict-origin-when-cross-origin");w.Header().Set("X-Frame-Options","DENY")
  w.Header().Set("Content-Security-Policy","default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'")
  mux.ServeHTTP(w,r)
 })
}
func main(){
 addr:=flag.String("listen","127.0.0.1:8080","HTTP listen address")
 dir:=flag.String("data-dir","./data","persistent catalog directory")
 source:=flag.String("source",sourceURL,"trusted upstream catalog URL")
 every:=flag.Duration("sync-interval",time.Hour,"background sync interval (0 disables)")
 once:=flag.Bool("sync-once",false,"refresh catalog and exit")
 shrink:=flag.Bool("allow-shrink",false,"permit >30% shrink (sync-once only)")
 flag.Parse()
 if *every<0||(*shrink&& !*once){log.Fatal("invalid sync options")}
 s:=&Store{dir:*dir}
 if err:=s.load();err!=nil{if !errors.Is(err,os.ErrNotExist){log.Printf("cached catalog rejected: %v",err)}}
 ctx,stop:=signal.NotifyContext(context.Background(),os.Interrupt,syscall.SIGTERM);defer stop()
 client:=&http.Client{Timeout:60*time.Second}
 syncNow:=func()error{return s.sync(ctx,client,*source,*shrink)}
 if *once{if err:=syncNow();err!=nil{log.Fatal(err)};return}
 done:=make(chan struct{})
 if *every>0{go func(){defer close(done);run:=func(){if err:=syncNow();err!=nil{log.Printf("sync failed: %v",err)}else{log.Printf("catalog updated: %s",s.current.Load().Catalog.Revision)}};run();ticker:=time.NewTicker(*every);defer ticker.Stop();for{select{case <-ctx.Done():return;case <-ticker.C:run()}}}()}else{close(done)}
 srv:=&http.Server{Addr:*addr,Handler:handler(s),ReadHeaderTimeout:5*time.Second,ReadTimeout:15*time.Second,WriteTimeout:30*time.Second,IdleTimeout:60*time.Second,MaxHeaderBytes:1<<20}
 go func(){<-ctx.Done();c,cancel:=context.WithTimeout(context.Background(),10*time.Second);defer cancel();_ = srv.Shutdown(c)}()
 log.Printf("LingXi Models listening on %s",*addr)
 if err:=srv.ListenAndServe();err!=nil&&!errors.Is(err,http.ErrServerClosed){log.Fatal(err)}
 stop();<-done
}
