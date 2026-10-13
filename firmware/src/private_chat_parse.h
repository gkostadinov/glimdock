#pragma once
#include "private_chat_model.h"
#include <ArduinoJson.h>
#include <new>
#include <strings.h>

namespace privatechat_json {
inline double timestamp(JsonVariantConst value){
  if(value.is<bool>()||value.is<const char*>()||!value.is<double>())return NAN;
  double result=value.as<double>();return std::isfinite(result)&&result>=0?result:NAN;
}
}
namespace privatechat_http {
inline bool headers(const char*input,size_t bytes,int&code,size_t&length){
  code=0;length=0;if(!input||bytes<16||bytes>privatechat::MAX_HTTP_HEADERS||memchr(input,0,bytes)||memcmp(input+bytes-4,"\r\n\r\n",4))return false;
  if(strncmp(input,"HTTP/1.1 ",9)&&strncmp(input,"HTTP/1.0 ",9))return false;
  if(input[9]<'0'||input[9]>'9'||input[10]<'0'||input[10]>'9'||input[11]<'0'||input[11]>'9'||input[12]!=' ')return false;code=(input[9]-'0')*100+(input[10]-'0')*10+input[11]-'0';
  const char*line=strstr(input,"\r\n");if(!line)return false;line+=2;bool hasLength=false,hasType=false;
  while(line<input+bytes-2){const char*end=strstr(line,"\r\n");if(!end)return false;if(end==line)break;const char*colon=(const char*)memchr(line,':',size_t(end-line));if(!colon||colon==line)return false;
    for(const char*p=line;p<colon;p++)if((unsigned char)*p<=32||(unsigned char)*p>=127)return false;
    const char*value=colon+1;while(value<end&&(*value==' '||*value=='\t'))value++;const char*last=end;while(last>value&&(last[-1]==' '||last[-1]=='\t'))last--;
    for(const char*p=value;p<last;p++)if((unsigned char)*p<32&&*p!='\t')return false;
    size_t name=size_t(colon-line);
    if(name==14&&!strncasecmp(line,"Content-Length",14)){if(hasLength||value==last)return false;hasLength=true;for(const char*p=value;p<last;p++){if(*p<'0'||*p>'9')return false;length=length*10+size_t(*p-'0');if(length>privatechat::MAX_JSON)return false;}}
    else if(name==17&&!strncasecmp(line,"Transfer-Encoding",17))return false;
    else if(name==16&&!strncasecmp(line,"Content-Encoding",16)){if(last-value!=8||strncasecmp(value,"identity",8))return false;}
    else if(name==12&&!strncasecmp(line,"Content-Type",12)){if(hasType||last-value<16||strncasecmp(value,"application/json",16)||(last-value>16&&value[16]!=';'&&value[16]!=' '))return false;hasType=true;}
    line=end+2;
  }
  return hasLength&&length>0&&hasType;
}
}
namespace privatechat_json {
inline bool integer(JsonVariantConst value,int64_t&out,bool nullable=true){
  if(value.isNull()){out=-1;return nullable;}
  if(value.is<bool>()||!value.is<int64_t>())return false;
  out=value.as<int64_t>();return out>=0;
}
// Validate each scalar and copy only whole UTF-8 codepoints. Identifiers/text
// continuations must fit exactly; short display labels may be clipped visibly.
template<size_t N>inline bool text(char(&out)[N],JsonVariantConst value,bool&clipped,bool exact=false){
  out[0]=0;if(value.isNull())return true;if(!value.is<const char*>())return false;
  JsonString str=value.as<JsonString>();const auto*input=reinterpret_cast<const uint8_t*>(str.c_str());size_t bytes=str.size(),written=0;bool full=false;
  for(size_t i=0;i<bytes;){uint8_t lead=input[i];size_t width=1;uint32_t point=lead;
    if(!lead)return false;
    if(lead>=0xc2&&lead<=0xdf){width=2;point=lead&0x1f;}
    else if(lead>=0xe0&&lead<=0xef){width=3;point=lead&0x0f;}
    else if(lead>=0xf0&&lead<=0xf4){width=4;point=lead&7;}
    else if(lead>=0x80)return false;
    if(i+width>bytes)return false;
    for(size_t j=1;j<width;j++){if((input[i+j]&0xc0)!=0x80)return false;point=(point<<6)|(input[i+j]&0x3f);}
    if((width==2&&point<0x80)||(width==3&&point<0x800)||(width==4&&point<0x10000)||point>0x10ffff||(point>=0xd800&&point<=0xdfff))return false;
    if(!full&&written+width<N){memcpy(out+written,input+i,width);written+=width;}
    else {full=true;clipped=true;if(exact)return false;}
    i+=width;
  }
  out[written]=0;return true;
}
template<size_t N>inline bool id(char(&out)[N],JsonVariantConst value){
  bool clipped=false;return value.is<const char*>()&&text(out,value,clipped,true)&&privatechat::validId(out,N-1);
}
inline bool parse(JsonDocument&document,privatechat::Model&model){
  auto root=document.as<JsonObjectConst>();
  if(!root||strcmp(root["schema_version"]|"","private-chat/v1")||(!root["demo"].isNull()&&!root["demo"].is<bool>()))return false;
  bool sessions=root["items"].is<JsonArrayConst>(),messages=root["messages"].is<JsonArrayConst>();
  if(sessions==messages)return false;
  privatechat::wipe(&model,sizeof(model));new(&model)privatechat::Model{};
  model.demo=root["demo"].as<bool>();model.generated=timestamp(root["generated_at"]);
  if(!integer(root["total"],model.total)||!root["has_more"].is<bool>())return false;model.hasMore=root["has_more"].as<bool>();
  bool clipped=false;
  if(!root["task_id"].isNull()||!root["system_id"].isNull()){if(!id(model.taskId,root["task_id"])||!id(model.systemId,root["system_id"]))return false;}
  if(sessions){
    model.kind=privatechat::Kind::Sessions;
    if(root["items"].size()>privatechat::MAX_SESSIONS)return false;
    if(!text(model.nextCursor,root["next_cursor"],clipped,true)||!privatechat::validCursor(model.nextCursor))return false;
    for(JsonVariantConst value:root["items"].as<JsonArrayConst>()){
      if(!value.is<JsonObjectConst>())return false;auto row=value.as<JsonObjectConst>();auto&session=model.sessions[model.nSessions];
      if(!id(session.id,row["id"])||!privatechat::validSessionId(session.id))return false;
      for(size_t i=0;i<model.nSessions;i++)if(!strcmp(model.sessions[i].id,session.id))return false;
      if(!text(session.title,row["title"],clipped)||!text(session.provider,row["provider"],clipped)||!text(session.node,row["machine"],clipped)||!text(session.project,row["project"],clipped))return false;
      session.startedAt=timestamp(row["started_at"]);session.updatedAt=timestamp(row["last_message_at"]);
      if(!integer(row["message_count"],session.messageCount))return false;model.nSessions++;
    }
  }else{
    model.kind=privatechat::Kind::Messages;
    auto session=root["session"].as<JsonObjectConst>();
    if(!session||!id(model.sessionId,session["id"])||!privatechat::validSessionId(model.sessionId)||root["messages"].size()>privatechat::MAX_MESSAGES||!integer(root["next_after"],model.nextAfter))return false;
    if(!text(model.selectedSessionTitle,session["title"],clipped)||!text(model.selectedSessionProvider,session["provider"],clipped)||!text(model.selectedSessionMachine,session["machine"],clipped))return false;
    int64_t previous=-1;
    for(JsonVariantConst value:root["messages"].as<JsonArrayConst>()){
      if(!value.is<JsonObjectConst>())return false;auto row=value.as<JsonObjectConst>();auto&message=model.messages[model.nMessages];int64_t order;
      if(!integer(row["order"],order,false)||order>UINT32_MAX||order<=previous)return false;message.order=uint32_t(order);previous=order;
      if(!text(message.id,row["id"],clipped,true)||!text(message.role,row["role"],clipped,true)||!text(message.text,row["text"],clipped,true)||!integer(row["next_text_offset"],message.nextTextOffset)||!integer(row["text_offset"],message.textOffset)||!integer(row["text_characters"],message.textCharacters))return false;
      if(message.textOffset<0)message.textOffset=0;
      if(row["redactions"].is<int64_t>()&&!row["redactions"].is<bool>())message.redactions=row["redactions"].as<int64_t>();
      if(strcmp(message.role,"user")&&strcmp(message.role,"assistant"))return false;
      message.timestamp=timestamp(row["timestamp"]);message.truncated=row["truncated"]==true||message.nextTextOffset>=0;model.nMessages++;
    }
  }
  model.truncated=clipped||root["truncated"]==true;model.valid=true;return true;
}
}
