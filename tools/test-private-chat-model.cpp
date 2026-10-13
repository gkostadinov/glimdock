// Synthetic private-reader contract and lifecycle checks; no real relay/files.
#include "private_chat_network.h"
#include "private_chat_parse.h"
#include <Arduino.h>
#include <ArduinoJson.h>
#include <cassert>
#include <iostream>
#include <string>

uint32_t millis(){return 1234;}
static int requests=0,lastKind=0;static std::string lastSession,lastCursor;static int64_t lastAfter=-2,lastOrder=-2,lastOffset=-2;static uint32_t lastGeneration=0;
extern "C" void homelabPrivateChatRequest(int kind,const char*session,const char*cursor,int64_t after,int64_t order,int64_t offset,uint32_t generation){requests++;lastKind=kind;lastSession=session;lastCursor=cursor;lastAfter=after;lastOrder=order;lastOffset=offset;lastGeneration=generation;}
static std::string sessionId(64,'a');
static JsonDocument sessions(){JsonDocument doc;doc["schema_version"]="private-chat/v1";doc["demo"]=true;doc["items"].to<JsonArray>();doc["total"]=1;doc["has_more"]=false;doc["next_cursor"]=nullptr;auto row=doc["items"].add<JsonObject>();row["id"]=sessionId;row["title"]="Synthetic conversation";row["machine"]="sample-machine";row["provider"]="sample-provider";row["last_message_at"]=nullptr;row["message_count"]=nullptr;return doc;}
static JsonDocument messages(){JsonDocument doc;doc["schema_version"]="private-chat/v1";doc["demo"]=true;doc["session"]["id"]=sessionId;doc["session"]["title"]="Synthetic conversation";doc["session"]["provider"]="sample-provider";doc["session"]["machine"]="sample-machine";doc["total"]=1;doc["has_more"]=false;doc["next_after"]=0;auto row=doc["messages"].add<JsonObject>();row["id"]="sample-message";row["order"]=0;row["role"]="assistant";row["timestamp"]=nullptr;row["text"]="Synthetic message only.";row["text_offset"]=0;row["text_characters"]=23;row["next_text_offset"]=nullptr;return doc;}
static std::string encoded(JsonDocument&doc){std::string result;serializeJson(doc,result);return result;}
int main(){
  char path[768];assert(privatechat::requestPath(path,sizeof(path),privatechat::Kind::Messages,sessionId.c_str(),"",-1));assert(std::string(path).find("after=")==std::string::npos);
  assert(privatechat::requestPath(path,sizeof(path),privatechat::Kind::Messages,sessionId.c_str(),"",4));assert(std::string(path).find("after=4")!=std::string::npos);
  assert(privatechat::requestPath(path,sizeof(path),privatechat::Kind::Messages,sessionId.c_str(),"",-1,5,2048));assert(std::string(path).find("message_order=5&text_offset=2048")!=std::string::npos);
  assert(!privatechat::requestPath(path,16,privatechat::Kind::Messages,sessionId.c_str(),"",-1));

  int status=0;size_t length=0;std::string response="HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2000\r\nConnection: close\r\n\r\n";assert(privatechat_http::headers(response.c_str(),response.size(),status,length)&&status==200&&length==2000);
  for(const char*bad:{"HTTP/1.1 200 OK\r\nContent-Length: 49153\r\nContent-Type: application/json\r\n\r\n","HTTP/1.1 200 OK\r\nContent-Length: 10\r\nContent-Length: 10\r\nContent-Type: application/json\r\n\r\n","HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: application/json\r\n\r\n","HTTP/1.1 200 OK\r\nContent-Length: 10\r\nContent-Type: text/html\r\n\r\n","HTTP/1.1 200 OK\r\nContent-Length: 10\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\n\r\n"})assert(!privatechat_http::headers(bad,strlen(bad),status,length));
  response="HTTP/1.1 200 OK\r\nX-Long: "+std::string(4096,'x')+"\r\nContent-Length: 10\r\nContent-Type: application/json\r\n\r\n";assert(!privatechat_http::headers(response.c_str(),response.size(),status,length));
  privatechat::Model model;privatechat::State state;
  auto doc=sessions();assert(privatechat_json::parse(doc,model));assert(model.valid&&model.demo&&model.kind==privatechat::Kind::Sessions&&model.nSessions==1);assert(std::isnan(model.generated)&&std::isnan(model.sessions[0].updatedAt)&&model.sessions[0].messageCount==-1);assert(!strcmp(model.sessions[0].node,"sample-machine"));
  doc=messages();doc["messages"][0]["timestamp"]=true;doc["messages"][0]["text_offset"]=200;doc["messages"][0]["next_text_offset"]=223;
  assert(privatechat_json::parse(doc,model));assert(model.kind==privatechat::Kind::Messages&&model.nMessages==1&&model.messages[0].textOffset==200&&model.messages[0].nextTextOffset==223&&std::isnan(model.messages[0].timestamp));assert(!strcmp(model.selectedSessionTitle,"Synthetic conversation"));
  doc=messages();doc["messages"][0]["text"]=std::string(privatechat::MAX_TEXT_BYTES,'x');assert(privatechat_json::parse(doc,model));doc["messages"][0]["text"]=std::string(privatechat::MAX_TEXT_BYTES+1,'x');assert(!privatechat_json::parse(doc,model));
  doc=messages();doc["messages"][0]["role"]="system";assert(!privatechat_json::parse(doc,model));
  doc=messages();doc["messages"][0]["order"]=-1;assert(!privatechat_json::parse(doc,model));
  doc=messages();doc["messages"][0]["text_offset"]=-1;assert(!privatechat_json::parse(doc,model));
  doc=messages();doc["messages"][0]["next_text_offset"]=false;assert(!privatechat_json::parse(doc,model));
  doc=sessions();doc["items"][0]["id"]="../../other";assert(!privatechat_json::parse(doc,model));
  doc=sessions();doc["items"][0]["id"]=std::string(64,'g');assert(!privatechat_json::parse(doc,model));
  doc=sessions();doc["next_cursor"]="cursor&limit=100";assert(!privatechat_json::parse(doc,model));
  doc=sessions();doc["items"].add(doc["items"][0]);assert(!privatechat_json::parse(doc,model));
  doc=sessions();for(int i=1;i<=int(privatechat::MAX_SESSIONS);i++){auto row=doc["items"].add<JsonObject>();row["id"]=std::string(63,'a')+"b";}assert(!privatechat_json::parse(doc,model));
  doc=messages();for(int i=1;i<=int(privatechat::MAX_MESSAGES);i++){auto row=doc["messages"].add<JsonObject>();row["order"]=i;row["role"]="user";row["text"]="Synthetic";}assert(!privatechat_json::parse(doc,model));
  doc=sessions();doc["items"][0]["title"]=std::string(190,'x')+"😀"+"z";assert(privatechat_json::parse(doc,model));assert(model.truncated&&strlen(model.sessions[0].title)==190&&model.sessions[0].title[189]=='x');
  doc=sessions();doc["items"][0]["title"]=std::string("bad\xc0\xaf");assert(!privatechat_json::parse(doc,model));
  doc=sessions();doc["items"][0]["title"]=std::string("hidden\0suffix",13);assert(!privatechat_json::parse(doc,model));
  initPrivateChat();assert(privateChatConfigured());assert(!requestPrivateSessions());assert(requests==0);setPrivateChatActive(true);assert(requestPrivateSessions("opaque_cursor"));assert(lastKind==1&&lastCursor=="opaque_cursor"&&requests==1);
  assert(consumePrivateChatViewRequest()==0);
  doc=sessions();auto text=encoded(doc);assert(ingestPrivateChat(text.c_str()));assert(readPrivateChat(model,state)&&state.active&&model.demo&&model.received==1234);assert(!state.loading);
  assert(requestPrivateMessages(sessionId.c_str(),-1));assert(lastKind==2&&lastAfter==-1&&lastSession==sessionId&&lastOrder==-1);assert(!readPrivateChat(model,state)&&state.loading);
  assert(requestPrivateMessages(sessionId.c_str(),-1,0,4096));assert(lastOrder==0&&lastOffset==4096);
  doc=messages();text=encoded(doc);assert(ingestPrivateChat(text.c_str()));assert(readPrivateChat(model,state));
  setPrivateChatActive(false);assert(!readPrivateChat(model,state)&&!state.active&&!state.loading&&model.nMessages==0&&model.messages[0].text[0]==0);int before=requests;assert(!requestPrivateMessages(sessionId.c_str()));assert(!ingestPrivateChat(text.c_str()));assert(requests==before);
  uint32_t staleGeneration=lastGeneration;setPrivateChatActive(true);assert(requestPrivateSessions());doc=sessions();text=encoded(doc);assert(!ingestPrivateChat(text.c_str(),staleGeneration));assert(!readPrivateChat(model,state)&&state.loading);assert(ingestPrivateChat(text.c_str(),lastGeneration));assert(readPrivateChat(model,state)&&model.kind==privatechat::Kind::Sessions);
  assert(requestPrivateMessages(sessionId.c_str()));doc=messages();doc["demo"]=false;text=encoded(doc);assert(!ingestPrivateChat(text.c_str(),lastGeneration));assert(!readPrivateChat(model,state));
  assert(!requestPrivateMessages(sessionId.c_str(),-2));assert(!requestPrivateMessages(sessionId.c_str(),0,1,-1));assert(!requestPrivateSessions("invalid?cursor"));
  std::cout<<"Private chat synthetic model/lifecycle checks passed.\n";
}
