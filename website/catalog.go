package main

import (
 "bytes"
 "context"
 "crypto/sha256"
 "encoding/hex"
 "encoding/json"
 "errors"
 "fmt"
 "io"
 "net/http"
 "os"
 "path/filepath"
 "sort"
 "strings"
 "sync"
 "sync/atomic"
 "time"
)

const sourceURL = "https://models.dev/api.json"
const maxCatalog = 128 << 20

type Object = map[string]json.RawMessage
type Catalog struct {
 SchemaVersion string `json:"schemaVersion"`
 Revision string `json:"catalogRevision"`
 Hash string `json:"catalogHash"`
 GeneratedAt string `json:"generatedAt"`
 Source string `json:"source"`
 SourceURL string `json:"sourceURL"`
 SourceFetchedAt string `json:"sourceFetchedAt"`
 SourceHash string `json:"sourceHash"`
 TotalProviders int `json:"totalProviders"`
 TotalModels int `json:"totalModels"`
 Providers map[string]Object `json:"providers"`
}
type Manifest struct {
 SchemaVersion string `json:"schemaVersion"`
 Revision string `json:"catalogRevision"`
 Hash string `json:"catalogHash"`
 GeneratedAt string `json:"generatedAt"`
 SourceFetchedAt string `json:"sourceFetchedAt"`
 SourceHash string `json:"sourceHash"`
 Providers int `json:"providers"`
 Models int `json:"models"`
 Bytes int `json:"bytes"`
 SHA256 string `json:"sha256"`
 PublishedAt string `json:"publishedAt"`
}
type Snapshot struct {
 Catalog Catalog
 Raw, Publication []byte
 ETag, PublicationETag string
 Providers []Provider
 Models []Model
}
type Store struct {
 dir string
 current atomic.Pointer[Snapshot]
 mu sync.Mutex
}
func hash(b []byte) string { h:=sha256.Sum256(b); return hex.EncodeToString(h[:]) }
func stamp(t time.Time) string { return t.UTC().Format(time.RFC3339) }
func encode(v any) ([]byte,error) {
 var b bytes.Buffer
 e:=json.NewEncoder(&b); e.SetEscapeHTML(false)
 if err:=e.Encode(v);err!=nil{return nil,err}
 return bytes.TrimSuffix(b.Bytes(),[]byte("\n")),nil
}
func raw(v any) json.RawMessage { b,_:=encode(v);return b }
func obj(b json.RawMessage) Object { var m Object; if json.Unmarshal(b,&m)!=nil{return nil};return m }
func str(b json.RawMessage) string {var s string;_ = json.Unmarshal(b,&s);return s}
func truth(b json.RawMessage) bool {return string(b)=="true"}
func optional(b json.RawMessage) bool {return len(b)==0||string(b)=="null"}
func validModel(m Object) bool {
 for _,k:=range []string{"reasoning","attachment","tool_call","structured_output","open_weights","temperature"} {
  if b:=m[k];!optional(b)&&string(b)!="true"&&string(b)!="false"{return false}
 }
 for _,k:=range []string{"id","name","description","family","canonical_model_id","release_date","last_updated","knowledge","status"} {
  if b:=m[k];!optional(b){var v string;if json.Unmarshal(b,&v)!=nil{return false}}
 }
 if b:=m["limit"];!optional(b){
  l:=obj(b);if l==nil{return false}
  for _,k:=range []string{"context","input","output"}{if v:=l[k];!optional(v){var n int64;if json.Unmarshal(v,&n)!=nil||n<0{return false}}}
 }
 if b:=m["cost"];!optional(b)&&obj(b)==nil{return false}
 if b:=m["modalities"];!optional(b){
  x:=obj(b);if x==nil{return false}
  for _,k:=range []string{"input","output"}{if v:=x[k];!optional(v){var a []json.RawMessage;if json.Unmarshal(v,&a)!=nil{return false};for _,r:=range a{var s string;if optional(r)||json.Unmarshal(r,&s)!=nil{return false}}}}
 }
 return true
}
func stripGuesses(o Object){for _,k:=range []string{"swiftDriver","reasoningField","swiftSnippet"}{delete(o,k)}}
func normalize(body []byte, url string, now time.Time, previous *Snapshot, allowShrink bool)([]byte,error){
 var source map[string]json.RawMessage
 if err:=json.Unmarshal(body,&source);err!=nil||len(source)==0{return nil,errors.New("invalid upstream document")}
 providers:=map[string]Object{}
 objects,withModels,total:=0,0,0
 for id,b:=range source {
  p:=obj(b);if p==nil{continue};objects++
  models:=obj(p["models"]);if len(models)==0{continue};withModels++
  kept:=Object{}
  for mid,r:=range models{m:=obj(r);if mid==""||m==nil||!validModel(m){continue};stripGuesses(m);kept[mid]=raw(m)}
  if id==""||len(kept)==0{continue}
  stripGuesses(p);p["models"]=raw(kept);p["modelCount"]=raw(len(kept))
  if u:=str(p["api"]);u!=""{p["baseURL"]=raw(u)}else{p["baseURL"]=raw(nil)}
  providers[id]=p;total+=len(kept)
 }
 if objects==0||withModels==0||withModels<objects/2||len(providers)==0||total==0{return nil,errors.New("upstream structure drift or empty catalog")}
 if previous!=nil&&!allowShrink{
  p:=previous.Catalog
  if float64(total)<0.7*float64(p.TotalModels)||float64(len(providers))<0.7*float64(p.TotalProviders){return nil,errors.New("catalog shrank by more than 30%; previous snapshot retained")}
 }
 // Canonical map encoding is stable across Go runs. Legacy Python revision may
 // differ on first migration due to float/Unicode serialization differences.
 canonical,err:=encode(map[string]any{"schemaVersion":"2.0","providers":providers});if err!=nil{return nil,err}
 digest:=hash(canonical)
 c:=Catalog{"2.0",digest[:12],"sha256:"+digest,stamp(now),"models.dev",url,stamp(now),"sha256:"+hash(body),len(providers),total,providers}
 return encode(c)
}
func parseSnapshot(body []byte,publishedAt string)(*Snapshot,error){
 var c Catalog
 if err:=json.Unmarshal(body,&c);err!=nil{return nil,err}
 if c.SchemaVersion!="2.0"||c.Revision==""||c.Hash==""||len(c.Providers)==0{return nil,errors.New("invalid catalog header")}
 count:=0
 for id,p:=range c.Providers{
  models:=obj(p["models"]);if id==""||len(models)==0{return nil,errors.New("invalid provider")}
  for mid,r:=range models{m:=obj(r);if mid==""||m==nil||!validModel(m){return nil,errors.New("invalid model")};count++}
 }
 if c.TotalModels!=count||c.TotalProviders!=len(c.Providers){return nil,errors.New("catalog count mismatch")}
 if publishedAt==""{publishedAt=c.GeneratedAt}
 m:=Manifest{c.SchemaVersion,c.Revision,c.Hash,c.GeneratedAt,c.SourceFetchedAt,c.SourceHash,c.TotalProviders,c.TotalModels,len(body),hash(body),publishedAt}
 publication,err:=encode(m);if err!=nil{return nil,err}
 s:=&Snapshot{Catalog:c,Raw:body,Publication:publication,ETag:`"`+hash(body)+`"`,PublicationETag:`"`+hash(publication)+`"`}
 s.buildIndex()
 return s,nil
}
func (s *Store) load() error {
 body,err:=os.ReadFile(filepath.Join(s.dir,"models.json"));if err!=nil{return err}
 snap,err:=parseSnapshot(body,"");if err!=nil{return err}
 // Preserve the old manifest's publication time when its checksum matches.
 if b,e:=os.ReadFile(filepath.Join(s.dir,"publication.json"));e==nil{
  var m Manifest
  if json.Unmarshal(b,&m)==nil&&m.SHA256==hash(body)&&m.Revision==snap.Catalog.Revision&&m.PublishedAt!=""{snap,_=parseSnapshot(body,m.PublishedAt)}
 }
 s.current.Store(snap);return nil
}
func writeAtomic(path string,b []byte) error {
 if err:=os.MkdirAll(filepath.Dir(path),0755);err!=nil{return err}
 f,err:=os.CreateTemp(filepath.Dir(path),".catalog-*");if err!=nil{return err}
 defer os.Remove(f.Name());defer f.Close()
 if err=f.Chmod(0644);err!=nil{return err}
 if _,err=f.Write(b);err!=nil{return err}
 if err=f.Sync();err!=nil{return err}
 if err=f.Close();err!=nil{return err}
 return os.Rename(f.Name(),path)
}
func (s *Store) sync(ctx context.Context,client *http.Client,url string,allowShrink bool) error{
 s.mu.Lock();defer s.mu.Unlock()
 var body []byte
 var err error
 for i:=0;i<3;i++{
  if i>0{select{case <-ctx.Done():return ctx.Err();case <-time.After(time.Duration(i)*2*time.Second):}}
  body,err=fetch(ctx,client,url);if err==nil{break}
 }
 if err!=nil{return err}
 now:=time.Now()
 b,err:=normalize(body,url,now,s.current.Load(),allowShrink);if err!=nil{return err}
 snap,err:=parseSnapshot(b,stamp(now));if err!=nil{return err}
 // One authoritative file is atomically replaced; the manifest is derived from
 // exactly those bytes in memory (and reconstructed on restart).
 if err=writeAtomic(filepath.Join(s.dir,"models.json"),b);err!=nil{return err}
 s.current.Store(snap)
 return nil
}
func fetch(ctx context.Context,client *http.Client,url string)([]byte,error){
 req,err:=http.NewRequestWithContext(ctx,http.MethodGet,url,nil);if err!=nil{return nil,err}
 req.Header.Set("Accept","application/json");req.Header.Set("User-Agent","LingXiModelSync/2.0 (https://models.lingxifox.cn)")
 resp,err:=client.Do(req);if err!=nil{return nil,err};defer resp.Body.Close()
 if resp.StatusCode!=http.StatusOK{return nil,fmt.Errorf("upstream HTTP %d",resp.StatusCode)}
 b,err:=io.ReadAll(io.LimitReader(resp.Body,maxCatalog+1));if err!=nil{return nil,err};if len(b)>maxCatalog{return nil,errors.New("upstream document too large")};return b,nil
}

type Provider struct{ ID,Name,Endpoint,Doc,Env string; Count int }
type Model struct{ ProviderID,ProviderName,ID,Name,Family,Description,Release,Updated,Status,Context,Output,InputPrice,OutputPrice string; ContextN int64; Reasoning,Vision,Attachments,Tools,Structured,OpenWeights bool }
func fallback(s,other string)string{if s==""{return other};return s}
func number(b json.RawMessage)int64{var n int64;_ = json.Unmarshal(b,&n);return n}
func displayNumber(b json.RawMessage)string{if optional(b){return "—"};n:=number(b);if n>=1000000{return fmt.Sprintf("%.1fM",float64(n)/1000000)};if n>=1000{return fmt.Sprintf("%.0fk",float64(n)/1000)};return fmt.Sprint(n)}
func displayPrice(b json.RawMessage)string{if optional(b){return "—"};var n float64;if json.Unmarshal(b,&n)!=nil{return "—"};return "$"+strings.TrimRight(strings.TrimRight(fmt.Sprintf("%.8f",n),"0"),".")}
func naturalLess(a,b string)bool{
 a,b=strings.ToLower(a),strings.ToLower(b)
 for len(a)>0&&len(b)>0 {
  if a[0]>='0'&&a[0]<='9'&&b[0]>='0'&&b[0]<='9'{
   i,j:=0,0;for i<len(a)&&a[i]>='0'&&a[i]<='9'{i++};for j<len(b)&&b[j]>='0'&&b[j]<='9'{j++}
   x,y:=strings.TrimLeft(a[:i],"0"),strings.TrimLeft(b[:j],"0")
   if len(x)!=len(y){return len(x)<len(y)};if x!=y{return x<y};a,b=a[i:],b[j:];continue
  }
  if a[0]!=b[0]{return a[0]<b[0]};a,b=a[1:],b[1:]
 }
 return len(a)<len(b)
}
func (s *Snapshot) buildIndex(){
 for id,p:=range s.Catalog.Providers{
  models:=obj(p["models"]);var env []string;_ = json.Unmarshal(p["env"],&env)
  s.Providers=append(s.Providers,Provider{id,fallback(str(p["name"]),id),fallback(str(p["baseURL"]),str(p["api"])),str(p["doc"]),strings.Join(env,", "),len(models)})
 }
 sort.Slice(s.Providers,func(i,j int)bool{a,b:=s.Providers[i],s.Providers[j];if naturalLess(a.Name,b.Name){return true};if naturalLess(b.Name,a.Name){return false};return a.ID<b.ID})
 for _,p:=range s.Providers{
  group:=[]Model{}
  for id,b:=range obj(s.Catalog.Providers[p.ID]["models"]){
   m:=obj(b);l,c,modal:=obj(m["limit"]),obj(m["cost"]),obj(m["modalities"]);var inputs []string;_ = json.Unmarshal(modal["input"],&inputs)
   vision:=false;for _,v:=range inputs{if strings.HasPrefix(strings.ToLower(v),"image"){vision=true}}
   group=append(group,Model{ProviderID:p.ID,ProviderName:p.Name,ID:id,Name:fallback(str(m["name"]),id),Family:str(m["family"]),Description:str(m["description"]),Release:str(m["release_date"]),Updated:str(m["last_updated"]),Status:str(m["status"]),Context:displayNumber(l["context"]),Output:displayNumber(l["output"]),InputPrice:displayPrice(c["input"]),OutputPrice:displayPrice(c["output"]),ContextN:number(l["context"]),Reasoning:truth(m["reasoning"]),Vision:vision,Attachments:truth(m["attachment"]),Tools:truth(m["tool_call"]),Structured:truth(m["structured_output"]),OpenWeights:truth(m["open_weights"])})
  }
  sort.Slice(group,func(i,j int)bool{a,b:=group[i],group[j];if a.Release!=b.Release{return a.Release>b.Release};for _,pair:=range [][2]string{{a.Family,b.Family},{a.Name,b.Name}}{if naturalLess(pair[0],pair[1]){return true};if naturalLess(pair[1],pair[0]){return false}};return a.ID<b.ID})
  s.Models=append(s.Models,group...)
 }
}
