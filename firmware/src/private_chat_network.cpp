#include "private_chat_network.h"
#include "flipper_usb.h"
#include "private_chat_parse.h"
#include <Arduino.h>
#include <ArduinoJson.h>
#include <new>

#ifdef HOMELAB_NATIVE_PREVIEW
// This transport is explicitly synthetic. It never reads NVS, credentials,
// private files or a relay; the emulator may provide a fixture-only callback.
namespace {
privatechat::Model nativeModel;
privatechat::State nativeState;
bool nativeExpectedBackward=false;privatechat::Kind nativeExpectedKind=privatechat::Kind::None;char nativeExpectedSession[97]{},nativeExpectedTask[65]{},nativeExpectedSystem[65]{};
void nativeClear(){privatechat::wipe(&nativeModel,sizeof(nativeModel));new(&nativeModel)privatechat::Model{};nativeModel.demo=true;nativeExpectedKind=privatechat::Kind::None;nativeExpectedBackward=false;privatechat::wipe(nativeExpectedSession,sizeof(nativeExpectedSession));privatechat::wipe(nativeExpectedTask,sizeof(nativeExpectedTask));privatechat::wipe(nativeExpectedSystem,sizeof(nativeExpectedSystem));}
}
extern "C" void __attribute__((weak)) homelabPrivateChatRequest(int,const char*,const char*,const char*,const char*,int64_t,int64_t,int64_t,bool,int64_t,uint32_t){}
void initPrivateChat(){uint32_t generation=nativeState.generation+1;nativeClear();nativeState=privatechat::State{};nativeState.generation=generation;nativeState.configured=true;strlcpy(nativeState.message,"Synthetic private reader preview.",sizeof(nativeState.message));}
void clearPrivateChatRuntime(){nativeClear();nativeState.generation++;nativeState.loading=false;nativeState.revision++;}
void setPrivateChatActive(bool active){if(nativeState.active==active)return;if(!active)clearPrivateChatRuntime();nativeState.active=active;nativeState.revision++;}
bool privateChatConfigured(){return nativeState.configured;}
bool requestPrivateSessions(const char*cursor,const char*taskId,const char*systemId){char path[768];if(!nativeState.active||!privatechat::requestPath(path,sizeof(path),privatechat::Kind::Sessions,"",cursor,-1,-1,-1,taskId,systemId))return false;clearPrivateChatRuntime();nativeExpectedKind=privatechat::Kind::Sessions;strlcpy(nativeExpectedTask,taskId,sizeof(nativeExpectedTask));strlcpy(nativeExpectedSystem,systemId,sizeof(nativeExpectedSystem));nativeState.loading=true;homelabPrivateChatRequest(1,"",cursor?cursor:"",taskId,systemId,0,-1,-1,false,-1,nativeState.generation);return true;}
bool requestPrivateMessages(const char*session,int64_t after,int64_t order,int64_t offset,bool newest,int64_t before){if(!nativeState.active||!privatechat::validSessionId(session)||after<-1||order<-1||offset<-1||before<-1||(order<0)!=(offset<0))return false;clearPrivateChatRuntime();nativeExpectedKind=privatechat::Kind::Messages;nativeExpectedBackward=newest;strlcpy(nativeExpectedSession,session,sizeof(nativeExpectedSession));nativeState.loading=true;homelabPrivateChatRequest(2,session,"","","",after,order,offset,newest,before,nativeState.generation);return true;}
bool readPrivateChat(privatechat::Model&model,privatechat::State&state){model=nativeModel;state=nativeState;return nativeModel.valid;}
uint32_t privateChatGeneration(){return nativeState.generation;}
bool ingestPrivateChat(const char*json,uint32_t generation){if(!nativeState.active||generation!=nativeState.generation||!json||strlen(json)>privatechat::MAX_JSON)return false;JsonDocument document;if(deserializeJson(document,json,DeserializationOption::NestingLimit(8))||document["demo"]!=true||!privatechat_json::parse(document,nativeModel)||(nativeExpectedKind!=privatechat::Kind::None&&nativeModel.kind!=nativeExpectedKind)||(nativeExpectedKind==privatechat::Kind::Sessions&&(strcmp(nativeModel.taskId,nativeExpectedTask)||strcmp(nativeModel.systemId,nativeExpectedSystem)))||(nativeExpectedKind==privatechat::Kind::Messages&&strcmp(nativeModel.sessionId,nativeExpectedSession))||(nativeExpectedKind==privatechat::Kind::Messages&&nativeModel.backward!=nativeExpectedBackward)){nativeClear();privateChatRuntimeError("Synthetic private page unavailable.");return false;}nativeModel.received=millis();nativeState.loading=false;nativeState.revision++;strlcpy(nativeState.message,"Synthetic private page.",sizeof(nativeState.message));return true;}
void privateChatRuntimeError(const char*message){nativeClear();nativeState.loading=false;nativeState.revision++;strlcpy(nativeState.message,message?message:"Synthetic private page unavailable.",sizeof(nativeState.message));}
void privateChatRuntimeError(const char*message,uint32_t generation){if(nativeState.active&&generation==nativeState.generation)privateChatRuntimeError(message);}
void privateChatProvisionTick(Stream&){}
void privateChatProvisionTick(){}
int consumePrivateChatViewRequest(){return 0;}
#else
#include <WiFi.h>
#include <WiFiClientSecure.h>
#include <Preferences.h>
#include <esp_heap_caps.h>
#include "runtime_health.h"
#include <cstddef>
#include <cctype>
#include <mbedtls/x509_crt.h>
#include <time.h>

namespace {
struct RelayConfig {char deviceId[65]{},url[257]{},token[257]{},ca[privatechat::MAX_CA_BYTES+1]{};};
struct Request {privatechat::Kind kind=privatechat::Kind::None;char session[97]{},cursor[513]{},taskId[65]{},systemId[65]{};int64_t after=-1,order=-1,offset=-1,before=-1;bool newest=false;uint32_t generation=0;};
SemaphoreHandle_t mutex=nullptr;TaskHandle_t task=nullptr;
privatechat::Model *published=nullptr,*working=nullptr;
privatechat::State state;
Request pending;
char *body=nullptr,*serialLine=nullptr;
size_t serialUsed=0;bool serialOverflow=false;
uint32_t serialBegan=0;
RelayConfig*relay=nullptr;
bool ready=false;
int viewRequest=0;

struct PsramAllocator:ArduinoJson::Allocator {
  struct alignas(std::max_align_t) Header {size_t size;};
  size_t allocated=0;static constexpr size_t limit=192*1024;
  void*allocate(size_t bytes)override{if(bytes>limit-allocated)return nullptr;auto*h=(Header*)heap_caps_malloc(bytes+sizeof(Header),MALLOC_CAP_SPIRAM|MALLOC_CAP_8BIT);if(!h)return nullptr;h->size=bytes;allocated+=bytes;return h+1;}
  void deallocate(void*memory)override{if(!memory)return;auto*h=((Header*)memory)-1;allocated-=h->size;privatechat::wipe(memory,h->size);heap_caps_free(h);}
  void*reallocate(void*memory,size_t bytes)override{if(!memory)return allocate(bytes);auto*h=((Header*)memory)-1;size_t old=h->size;if(bytes>limit-(allocated-old))return nullptr;void*next=allocate(bytes);if(!next)return nullptr;memcpy(next,memory,old<bytes?old:bytes);deallocate(memory);return next;}
};
bool printable(const char*value,size_t low,size_t high){if(!value)return false;size_t n=strlen(value);if(n<low||n>high)return false;for(size_t i=0;i<n;i++)if((unsigned char)value[i]<33||(unsigned char)value[i]>126)return false;return true;}
bool endpoint(const char*url,IPAddress&address,uint16_t&port){
  if(!printable(url,9,256)||strncmp(url,"https://",8))return false;const char*authority=url+8;const char*end=strchr(authority,'/');if(end&&strcmp(end,"/"))return false;size_t n=end?size_t(end-authority):strlen(authority);const char*colon=(const char*)memchr(authority,':',n);size_t hostSize=colon?size_t(colon-authority):n;if(!hostSize||hostSize>15)return false;
  char host[16]{};memcpy(host,authority,hostSize);if(!address.fromString(host)||address.toString()!=String(host))return false;port=443;
  if(colon){if(colon+1>=authority+n)return false;unsigned value=0;for(const char*p=colon+1;p<authority+n;p++){if(*p<'0'||*p>'9')return false;value=value*10+unsigned(*p-'0');if(value>65535)return false;}if(!value)return false;port=uint16_t(value);}return true;
}
bool relayUrl(const char*url){IPAddress address;uint16_t port;return endpoint(url,address,port);}
// Split the 1.2-second connect budget between TCP and TLS. A literal IP avoids
// the framework's unbounded DNS path, while its TLS layer verifies the IP SAN.
class BoundedTlsClient:public WiFiClientSecure {public:int connectRelay(IPAddress address,uint16_t port){_timeout=600;sslclient->handshake_timeout=600;return WiFiClientSecure::connect(address,port);}};
bool configValid(const RelayConfig&config){if(!printable(config.deviceId,1,64)||!relayUrl(config.url)||!printable(config.token,24,256)||!strstr(config.ca,"-----BEGIN CERTIFICATE-----")||!strstr(config.ca,"-----END CERTIFICATE-----")||strstr(config.ca,"PRIVATE KEY"))return false;mbedtls_x509_crt certificate;mbedtls_x509_crt_init(&certificate);bool valid=mbedtls_x509_crt_parse(&certificate,reinterpret_cast<const unsigned char*>(config.ca),strlen(config.ca)+1)==0;mbedtls_x509_crt_free(&certificate);return valid;}
bool loadConfig(RelayConfig&config){
  privatechat::wipe(&config,sizeof(config));Preferences preferences;if(!preferences.begin("privateChat",true))return false;
  bool committed=preferences.getBool("ready",false);
  const char*keys[]={"device_id","relay_url","device_token","ca_pem"};char*values[]={config.deviceId,config.url,config.token,config.ca};const size_t caps[]={sizeof(config.deviceId),sizeof(config.url),sizeof(config.token),sizeof(config.ca)};
  bool valid=committed;
  for(size_t i=0;i<4&&valid;i++){size_t n=0;if(i==3&&preferences.getType(keys[i])==PT_BLOB){size_t size=preferences.getBytesLength(keys[i]);if(size>=2&&size<=caps[i])n=preferences.getBytes(keys[i],values[i],caps[i]);}else n=preferences.getString(keys[i],values[i],caps[i]);if(n<2||n>caps[i]||values[i][n-1]!=0||strlen(values[i])+1!=n)valid=false;}
  preferences.end();if(!valid||!configValid(config)){privatechat::wipe(&config,sizeof(config));return false;}
  size_t n=strlen(config.url);if(config.url[n-1]=='/')config.url[n-1]=0;return true;
}
bool saveConfig(const RelayConfig&config){
  Preferences preferences;if(!preferences.begin("privateChat",false))return false;
  bool ok=preferences.putBool("ready",false)>0;
  const char*keys[]={"device_id","relay_url","device_token","ca_pem"};const char*values[]={config.deviceId,config.url,config.token,config.ca};
  // NVS strings cannot span pages; the CA uses a bounded blob to support 8 KiB.
  for(size_t i=0;i<4&&ok;i++)ok=i==3?preferences.putBytes(keys[i],values[i],strlen(values[i])+1)==strlen(values[i])+1:preferences.putString(keys[i],values[i])==strlen(values[i]);
  if(ok)ok=preferences.putBool("ready",true)>0;preferences.end();
  auto*check=(RelayConfig*)heap_caps_malloc(sizeof(RelayConfig),MALLOC_CAP_SPIRAM|MALLOC_CAP_8BIT);
  if(check){new(check)RelayConfig{};ok=ok&&loadConfig(*check)&&!strcmp(check->deviceId,config.deviceId)&&!strcmp(check->url,config.url)&&!strcmp(check->token,config.token)&&!strcmp(check->ca,config.ca);privatechat::wipe(check,sizeof(*check));heap_caps_free(check);}else ok=false;
  return ok;
}
void resetModel(privatechat::Model*model){if(model){privatechat::wipe(model,sizeof(*model));new(model)privatechat::Model{};}}
void allocationFailure(){heap_caps_free(published);heap_caps_free(working);heap_caps_free(body);heap_caps_free(serialLine);heap_caps_free(relay);published=working=nullptr;body=serialLine=nullptr;relay=nullptr;if(mutex){vSemaphoreDelete(mutex);mutex=nullptr;}}
bool current(uint32_t generation){xSemaphoreTake(mutex,portMAX_DELAY);bool yes=state.active&&state.configured&&state.generation==generation;xSemaphoreGive(mutex);return yes;}
void result(const char*message,uint32_t generation){xSemaphoreTake(mutex,portMAX_DELAY);if(generation==state.generation){state.loading=false;state.revision++;strlcpy(state.message,message,sizeof(state.message));Serial.printf("Private reader status: %s\n",message);}xSemaphoreGive(mutex);}
bool fetch(const Request&request){
  if(!current(request.generation)||WiFi.status()!=WL_CONNECTED){result("Private reader unavailable while Wi-Fi is offline.",request.generation);return false;}
  if(!loadConfig(*relay)){result("Private reader pairing is unavailable.",request.generation);return false;}
  if(!current(request.generation))return false;
  if(time(nullptr)<1700000000){result("Private reader clock synchronization pending.",request.generation);return false;}
  IPAddress address;uint16_t port;if(!endpoint(relay->url,address,port)){result("Private reader requires a literal HTTPS IPv4 origin.",request.generation);return false;}
  char path[768]{};if(!privatechat::requestPath(path,sizeof(path),request.kind,request.session,request.cursor,request.after,request.order,request.offset,request.taskId,request.systemId,request.newest,request.before)){result("Private reader request is invalid.",request.generation);return false;}
  BoundedTlsClient client;client.setCACert(relay->ca);if(!current(request.generation)||!client.connectRelay(address,port)){result("Private reader connection unavailable.",request.generation);return false;}client.setTimeout(2);
  if(!current(request.generation)){client.stop();return false;}
  char wire[2048]{};int wireSize=snprintf(wire,sizeof(wire),"GET %s HTTP/1.1\r\nHost: %s\r\nAccept: application/json\r\nConnection: close\r\nAuthorization: Bearer %s\r\n\r\n",path,relay->url+8,relay->token);
  privatechat::wipe(path,sizeof(path));
  if(wireSize<=0||wireSize>=int(sizeof(wire))){privatechat::wipe(wire,sizeof(wire));client.stop();result("Private reader request exceeded its bounds.",request.generation);return false;}
  size_t expected=size_t(wireSize),sent=client.write(reinterpret_cast<const uint8_t*>(wire),expected);privatechat::wipe(wire,sizeof(wire));
  if(sent!=expected||!current(request.generation)){client.stop();result("Private reader request changed or unavailable.",request.generation);return false;}
  uint32_t began=millis();char responseHeaders[privatechat::MAX_HTTP_HEADERS+1]{};size_t headerSize=0;bool complete=false;
  while(headerSize<privatechat::MAX_HTTP_HEADERS&&millis()-began<2000&&current(request.generation)){
    if(client.available()<=0){if(!client.connected())break;vTaskDelay(pdMS_TO_TICKS(5));continue;}int value=client.read();if(value<0)break;responseHeaders[headerSize++]=char(value);if(headerSize>=4&&!memcmp(responseHeaders+headerSize-4,"\r\n\r\n",4)){complete=true;break;}
  }
  int code=0;size_t size=0;bool headerOk=complete&&privatechat_http::headers(responseHeaders,headerSize,code,size);privatechat::wipe(responseHeaders,sizeof(responseHeaders));
  if(!headerOk||code!=200){client.stop();result(code==401||code==403?"Private reader pairing was not accepted.":"Private reader response unavailable or invalid.",request.generation);return false;}
  size_t used=0;uint8_t chunk[512];bool valid=true;
  while(used<size){
    if(!current(request.generation)||millis()-began>=2000){valid=false;break;}
    int available=client.available();if(available<=0){if(!client.connected()){valid=false;break;}vTaskDelay(pdMS_TO_TICKS(5));continue;}
    size_t count=size_t(available);if(count>sizeof(chunk))count=sizeof(chunk);if(count>size-used)count=size-used;
    int read=client.read(chunk,count);if(read<=0){valid=false;break;}
    xSemaphoreTake(mutex,portMAX_DELAY);if(state.active&&state.generation==request.generation){memcpy(body+used,chunk,size_t(read));used+=size_t(read);}else valid=false;xSemaphoreGive(mutex);privatechat::wipe(chunk,sizeof(chunk));if(!valid)break;
  }
  client.stop();privatechat::wipe(chunk,sizeof(chunk));
  if(!valid||used!=size){result("Private reader request timed out or changed.",request.generation);return false;}
  xSemaphoreTake(mutex,portMAX_DELAY);
  bool accepted=state.active&&state.generation==request.generation;
  if(accepted){body[used]=0;PsramAllocator allocator;JsonDocument document(&allocator);
    accepted=!deserializeJson(document,(const char*)body,used,DeserializationOption::NestingLimit(8))&&privatechat_json::parse(document,*working)&&working->kind==request.kind;
    if(accepted&&request.kind==privatechat::Kind::Sessions)accepted=!strcmp(working->taskId,request.taskId)&&!strcmp(working->systemId,request.systemId);
    if(accepted&&request.kind==privatechat::Kind::Messages)accepted=!strcmp(working->sessionId,request.session)&&working->backward==request.newest;
    if(accepted){working->received=millis();memcpy(published,working,sizeof(*published));state.loading=false;state.revision++;strlcpy(state.message,"Private conversation page ready.",sizeof(state.message));if(working->kind==privatechat::Kind::Sessions)Serial.printf("Private reader: sessions=%u\n",unsigned(working->nSessions));else {bool continued=false;for(size_t i=0;i<working->nMessages;i++)if(working->messages[i].nextTextOffset>=0)continued=true;Serial.printf("Private reader: messages=%u, continued=%s\n",unsigned(working->nMessages),continued?"yes":"no");}}
  }
  privatechat::wipe(body,privatechat::MAX_JSON+1);resetModel(working);xSemaphoreGive(mutex);
  if(!accepted)result("Private reader response unavailable or invalid.",request.generation);return accepted;
}
void worker(void*){
  for(;;){ulTaskNotifyTake(pdTRUE,portMAX_DELAY);Request request;
    xSemaphoreTake(mutex,portMAX_DELAY);request=pending;privatechat::wipe(&pending,sizeof(pending));new(&pending)Request{};bool execute=state.active&&state.configured&&request.kind!=privatechat::Kind::None&&request.generation==state.generation;xSemaphoreGive(mutex);
    if(execute)fetch(request);
    xSemaphoreTake(mutex,portMAX_DELAY);privatechat::wipe(body,privatechat::MAX_JSON+1);resetModel(working);privatechat::wipe(relay,sizeof(*relay));xSemaphoreGive(mutex);privatechat::wipe(&request,sizeof(request));
  }
}
bool queue(Request&request){if(!ready)return false;xSemaphoreTake(mutex,portMAX_DELAY);bool allowed=state.active&&state.configured;if(allowed){state.generation++;request.generation=state.generation;pending=request;resetModel(published);state.loading=true;state.revision++;strlcpy(state.message,"Loading private conversation page…",sizeof(state.message));}xSemaphoreGive(mutex);if(allowed)xTaskNotifyGive(task);return allowed;}
bool configText(char*to,size_t cap,JsonVariantConst value){if(!value.is<const char*>())return false;JsonString input=value.as<JsonString>();if(!input.size()||input.size()>=cap||strlen(input.c_str())!=input.size())return false;memcpy(to,input.c_str(),input.size()+1);return true;}
void provision(Stream&serial){
  if(!serialOverflow&&handleRuntimeHealthLine(serialLine,serialUsed,serial)){privatechat::wipe(serialLine,privatechat::MAX_CONFIG_JSON+1);serialUsed=0;return;}
  if(!serialOverflow&&flipper::handleUsbLine(serialLine,serialUsed,serial)){privatechat::wipe(serialLine,privatechat::MAX_CONFIG_JSON+1);serialUsed=0;serialOverflow=false;return;}
  constexpr char prefix[]="GLIMDOCK_PRIVATE_READER ";char nonce[49]{};bool success=false;
  if(!serialOverflow&&!strcmp(serialLine,"GLIMDOCK_PRIVATE_READER_OPEN")){viewRequest=1;privatechat::wipe(serialLine,privatechat::MAX_CONFIG_JSON+1);serialUsed=0;return;}
  if(!serialOverflow&&!strcmp(serialLine,"GLIMDOCK_PRIVATE_READER_CLOSE")){setPrivateChatActive(false);clearPrivateChatRuntime();viewRequest=2;privatechat::wipe(serialLine,privatechat::MAX_CONFIG_JSON+1);serialUsed=0;return;}
  if(!serialOverflow&&!strcmp(serialLine,"GLIMDOCK_PRIVATE_READER_FIRST")){viewRequest=3;privatechat::wipe(serialLine,privatechat::MAX_CONFIG_JSON+1);serialUsed=0;return;}
  if(!serialOverflow&&serialUsed>sizeof(prefix)-1&&!strncmp(serialLine,prefix,sizeof(prefix)-1)){
    PsramAllocator allocator;JsonDocument document(&allocator);
    auto*candidate=(RelayConfig*)heap_caps_malloc(sizeof(RelayConfig),MALLOC_CAP_SPIRAM|MALLOC_CAP_8BIT);
    if(candidate){new(candidate)RelayConfig{};
      if(!deserializeJson(document,(const char*)serialLine+sizeof(prefix)-1,serialUsed-(sizeof(prefix)-1),DeserializationOption::NestingLimit(4))){
        auto root=document.as<JsonObjectConst>();bool fields=bool(root);
        for(JsonPairConst field:root){const char*name=field.key().c_str();if(strcmp(name,"schema_version")&&strcmp(name,"device_id")&&strcmp(name,"relay_url")&&strcmp(name,"device_token")&&strcmp(name,"ca_pem")&&strcmp(name,"nonce"))fields=false;}
        bool nonceValid=configText(nonce,sizeof(nonce),root["nonce"])&&strlen(nonce)>=16&&privatechat::validCursor(nonce);
        if(!nonceValid)nonce[0]=0;
        bool valid=fields&&nonceValid&&!strcmp(root["schema_version"]|"","private-chat-pairing/v1")&&configText(candidate->deviceId,sizeof(candidate->deviceId),root["device_id"])&&configText(candidate->url,sizeof(candidate->url),root["relay_url"])&&configText(candidate->token,sizeof(candidate->token),root["device_token"])&&configText(candidate->ca,sizeof(candidate->ca),root["ca_pem"])&&configValid(*candidate);
        if(valid){size_t n=strlen(candidate->url);if(candidate->url[n-1]=='/')candidate->url[n-1]=0;clearPrivateChatRuntime();success=saveConfig(*candidate);xSemaphoreTake(mutex,portMAX_DELAY);state.configured=success;state.loading=false;state.revision++;strlcpy(state.message,success?"Private reader paired over USB.":"Private reader pairing could not be saved.",sizeof(state.message));xSemaphoreGive(mutex);}
      }
      privatechat::wipe(candidate,sizeof(*candidate));heap_caps_free(candidate);
    }
  }
  serial.print(success?"GLIMDOCK_PRIVATE_READER_OK ":"GLIMDOCK_PRIVATE_READER_ERROR ");serial.println(nonce[0]?nonce:"-");privatechat::wipe(nonce,sizeof(nonce));privatechat::wipe(serialLine,privatechat::MAX_CONFIG_JSON+1);serialUsed=0;serialOverflow=false;
}
}

void initPrivateChat(){
  if(ready)return;mutex=xSemaphoreCreateMutex();
  published=(privatechat::Model*)heap_caps_malloc(sizeof(privatechat::Model),MALLOC_CAP_SPIRAM|MALLOC_CAP_8BIT);working=(privatechat::Model*)heap_caps_malloc(sizeof(privatechat::Model),MALLOC_CAP_SPIRAM|MALLOC_CAP_8BIT);
  body=(char*)heap_caps_malloc(privatechat::MAX_JSON+1,MALLOC_CAP_SPIRAM|MALLOC_CAP_8BIT);serialLine=(char*)heap_caps_malloc(privatechat::MAX_CONFIG_JSON+1,MALLOC_CAP_SPIRAM|MALLOC_CAP_8BIT);relay=(RelayConfig*)heap_caps_malloc(sizeof(RelayConfig),MALLOC_CAP_SPIRAM|MALLOC_CAP_8BIT);
  if(!mutex||!published||!working||!body||!serialLine||!relay){allocationFailure();strlcpy(state.message,"Private reader memory unavailable.",sizeof(state.message));return;}
  resetModel(published);resetModel(working);privatechat::wipe(body,privatechat::MAX_JSON+1);privatechat::wipe(serialLine,privatechat::MAX_CONFIG_JSON+1);new(relay)RelayConfig{};state.configured=loadConfig(*relay);privatechat::wipe(relay,sizeof(*relay));if(state.configured)strlcpy(state.message,"Private reader ready; open conversations to connect.",sizeof(state.message));
  if(xTaskCreatePinnedToCore(worker,"private-chat",24576,nullptr,1,&task,0)!=pdPASS){allocationFailure();state.configured=false;strlcpy(state.message,"Private reader worker unavailable.",sizeof(state.message));return;}ready=true;
}
void clearPrivateChatRuntime(){if(!ready)return;xSemaphoreTake(mutex,portMAX_DELAY);state.generation++;state.loading=false;state.revision++;privatechat::wipe(&pending,sizeof(pending));new(&pending)Request{};resetModel(published);resetModel(working);privatechat::wipe(body,privatechat::MAX_JSON+1);xSemaphoreGive(mutex);}
void setPrivateChatActive(bool active){if(!ready)return;xSemaphoreTake(mutex,portMAX_DELAY);bool changed=state.active!=active;xSemaphoreGive(mutex);if(!changed)return;if(!active)clearPrivateChatRuntime();xSemaphoreTake(mutex,portMAX_DELAY);state.active=active;state.revision++;xSemaphoreGive(mutex);}
bool privateChatConfigured(){if(!ready)return false;xSemaphoreTake(mutex,portMAX_DELAY);bool yes=state.configured;xSemaphoreGive(mutex);return yes;}
bool requestPrivateSessions(const char*cursor,const char*taskId,const char*systemId){char path[768];if(!privatechat::requestPath(path,sizeof(path),privatechat::Kind::Sessions,"",cursor,-1,-1,-1,taskId,systemId))return false;Request request;request.kind=privatechat::Kind::Sessions;if(cursor)strlcpy(request.cursor,cursor,sizeof(request.cursor));strlcpy(request.taskId,taskId,sizeof(request.taskId));strlcpy(request.systemId,systemId,sizeof(request.systemId));return queue(request);}
bool requestPrivateMessages(const char*session,int64_t after,int64_t order,int64_t offset,bool newest,int64_t before){if(!privatechat::validSessionId(session)||after<-1||order<-1||offset<-1||before<-1||(order<0)!=(offset<0))return false;Request request;request.kind=privatechat::Kind::Messages;strlcpy(request.session,session,sizeof(request.session));request.after=after;request.order=order;request.offset=offset;request.newest=newest;request.before=before;return queue(request);}
bool readPrivateChat(privatechat::Model&model,privatechat::State&out){if(!ready){privatechat::wipe(&model,sizeof(model));new(&model)privatechat::Model{};out=state;return false;}xSemaphoreTake(mutex,portMAX_DELAY);memcpy(&model,published,sizeof(model));out=state;bool valid=model.valid;xSemaphoreGive(mutex);return valid;}
void privateChatProvisionTick(Stream&serial){if(!ready)return;if((serialUsed||serialOverflow)&&millis()-serialBegan>5000){privatechat::wipe(serialLine,privatechat::MAX_CONFIG_JSON+1);serialUsed=0;serialOverflow=false;}size_t count=0;while(serial.available()&&count++<512){int value=serial.read();if(value<0)break;if(value=='\n'){provision(serial);continue;}if(!serialUsed&&!serialOverflow)serialBegan=millis();if(serialUsed>=privatechat::MAX_CONFIG_JSON){serialOverflow=true;continue;}serialLine[serialUsed++]=char(value);serialLine[serialUsed]=0;}}
void privateChatProvisionTick(){privateChatProvisionTick(Serial);}
int consumePrivateChatViewRequest(){int request=viewRequest;viewRequest=0;if(request)Serial.printf("Private reader view command=%d\n",request);return request;}
#endif
