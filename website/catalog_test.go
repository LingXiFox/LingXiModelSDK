package main
import (
 "bytes"
 "context"
 "encoding/json"
 "net/http"
 "net/http/httptest"
 "net/url"
 "os"
 "path/filepath"
 "strings"
 "testing"
 "time"
)
const fixture=`{"p":{"name":"Provider 2","api":"https://example.com/v1","env":["KEY"],"unknown":{"x":1},"models":{"m":{"name":"Model","limit":{"context":128000},"cost":{"input":0,"output":0.000001},"reasoning":true,"attachment":true,"modalities":{"input":["text"]},"extra":"keep"}}}}`
func snapshot(t *testing.T)*Snapshot{t.Helper();b,e:=normalize([]byte(fixture),sourceURL,time.Unix(1,0),nil,false);if e!=nil{t.Fatal(e)};s,e:=parseSnapshot(b,"");if e!=nil{t.Fatal(e)};return s}
func TestNormalize(t *testing.T){
 s:=snapshot(t);m:=s.Models[0]
 if s.Catalog.TotalModels!=1||m.InputPrice!="$0"||m.OutputPrice!="$0.000001"||m.Vision||!m.Attachments||m.Output!="—"{t.Fatalf("%+v",m)}
 if !bytes.Contains(s.Raw,[]byte(`"extra":"keep"`))||!bytes.Contains(s.Raw,[]byte(`"unknown"`)){t.Fatal("unknown metadata lost")}
 b,e:=normalize([]byte(fixture),sourceURL,time.Unix(2,0),nil,false);if e!=nil{t.Fatal(e)};s2,e:=parseSnapshot(b,"");if e!=nil||s.Catalog.Revision!=s2.Catalog.Revision{t.Fatal("unstable revision",e)}
 if _,e=normalize([]byte(`{}`),sourceURL,time.Now(),s,false);e==nil{t.Fatal("empty source accepted")}
}
func TestHTTP(t *testing.T){
 s:=&Store{};snap:=snapshot(t);s.current.Store(snap);h:=handler(s)
 for _,p:=range []string{"/","/models.json","/publication.json","/healthz","/static/site.css","/static/site.js"}{r:=httptest.NewRequest("GET",p,nil);w:=httptest.NewRecorder();h.ServeHTTP(w,r);if w.Code!=200{t.Fatalf("%s: %d %s",p,w.Code,w.Body.String())}}
 r:=httptest.NewRequest("GET","/models.json",nil);r.Header.Set("If-None-Match",snap.ETag);w:=httptest.NewRecorder();h.ServeHTTP(w,r);if w.Code!=304{t.Fatalf("ETag: %d",w.Code)}
 w=httptest.NewRecorder();h.ServeHTTP(w,httptest.NewRequest("GET","/publication.json",nil));var m Manifest;if json.Unmarshal(w.Body.Bytes(),&m)!=nil||m.SHA256!=hash(snap.Raw){t.Fatal("manifest mismatch")}
 w=httptest.NewRecorder();h.ServeHTTP(w,httptest.NewRequest("HEAD","/models.json",nil));if w.Code!=200||w.Body.Len()!=0{t.Fatal("HEAD")}
 w=httptest.NewRecorder();h.ServeHTTP(w,httptest.NewRequest("GET","/?q=does-not-exist",nil));if !strings.Contains(w.Body.String(),"未找到"){t.Fatal("filter")}
 if v:=view(snap,url.Values{"cap":{"vision"}});v.Total!=0{t.Fatal("vision filter")}
 if v:=view(snap,url.Values{"cap":{"reasoning"}});v.Total!=1{t.Fatal("reasoning filter")}
}
func TestSyncRetainsSnapshot(t *testing.T){
 body:=fixture;up:=httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter,r *http.Request){_,_=w.Write([]byte(body))}));defer up.Close()
 s:=&Store{dir:t.TempDir()};if e:=s.sync(context.Background(),up.Client(),up.URL,false);e!=nil{t.Fatal(e)};old:=s.current.Load()
 body=`{}`;if e:=s.sync(context.Background(),up.Client(),up.URL,false);e==nil{t.Fatal("expected failure")}
 b,e:=os.ReadFile(filepath.Join(s.dir,"models.json"));if e!=nil||!bytes.Equal(b,old.Raw)||s.current.Load()!=old{t.Fatal("last known good lost")}
 second:=&Store{dir:s.dir};if e=second.load();e!=nil||second.current.Load().Catalog.Revision!=old.Catalog.Revision{t.Fatal("restart failed",e)}
}
