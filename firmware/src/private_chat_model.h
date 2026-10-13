#pragma once
#include <cstddef>
#include <cstdint>
#include <cmath>
#include <cstring>
#include <cstdio>

// Private pages live in a separate runtime model, never in telemetry snapshots.
namespace privatechat {
constexpr size_t MAX_JSON=48*1024,MAX_SESSIONS=12,MAX_MESSAGES=5,MAX_TEXT_BYTES=4096;
constexpr size_t MAX_HTTP_HEADERS=4096;
constexpr size_t MAX_CONFIG_JSON=12*1024,MAX_CA_BYTES=8192;
enum class Kind:uint8_t {None=0,Sessions=1,Messages=2};
struct Session {
  char id[97]{},title[193]{},provider[24]{},node[24]{},project[129]{};
  double startedAt=NAN,updatedAt=NAN;int64_t messageCount=-1;
};
struct Message {
  uint32_t order=0;char id[97]{},role[16]{},text[MAX_TEXT_BYTES+1]{};
  double timestamp=NAN;int64_t textOffset=0,textCharacters=-1,nextTextOffset=-1,redactions=-1;bool truncated=false;
};
struct Model {
  bool valid=false,demo=false,truncated=false;Kind kind=Kind::None;
  double generated=NAN;uint32_t received=0;
  char sessionId[97]{},nextCursor[513]{},selectedSessionTitle[193]{},selectedSessionProvider[24]{},selectedSessionMachine[24]{};
  int64_t nextAfter=-1,total=-1;bool hasMore=false;
  uint8_t nSessions=0,nMessages=0;Session sessions[MAX_SESSIONS];Message messages[MAX_MESSAGES];
};
struct State {
  bool configured=false,active=false,loading=false;
  uint32_t revision=0,generation=0;char message[96]{"Private reader is not configured."};
};
inline bool validId(const char*id,size_t max=96){
  if(!id||!id[0])return false;size_t i=0;
  for(;id[i];i++){unsigned char c=id[i];if(i>=max||!((c>='a'&&c<='z')||(c>='A'&&c<='Z')||(c>='0'&&c<='9')||c==':'||c=='_'||c=='-'||c=='.'))return false;}
  return true;
}
inline bool validSessionId(const char*id){if(!id||strlen(id)!=64)return false;for(size_t i=0;i<64;i++)if(!((id[i]>='0'&&id[i]<='9')||(id[i]>='a'&&id[i]<='f')))return false;return true;}
inline bool validCursor(const char*cursor){
  if(!cursor)return true;
  for(size_t i=0;cursor[i];i++){unsigned char c=cursor[i];if(i>=512||!((c>='a'&&c<='z')||(c>='A'&&c<='Z')||(c>='0'&&c<='9')||c=='_'||c=='-'))return false;}
  return true;
}
inline bool requestPath(char*out,size_t cap,Kind kind,const char*session,const char*cursor,int64_t after=-1,int64_t order=-1,int64_t offset=-1){
  if(!out||!cap)return false;out[0]=0;int n=-1;
  if(kind==Kind::Sessions&&validCursor(cursor))n=snprintf(out,cap,"/v1/conversations?limit=12%s%s",cursor&&cursor[0]?"&cursor=":"",cursor?cursor:"");
  else if(kind==Kind::Messages&&validSessionId(session)&&after>=-1&&order>=-1&&offset>=-1&&(order<0)==(offset<0)){
    if(order>=0)n=snprintf(out,cap,"/v1/conversations/%s/messages?limit=5&message_order=%lld&text_offset=%lld",session,(long long)order,(long long)offset);
    else if(after>=0)n=snprintf(out,cap,"/v1/conversations/%s/messages?limit=5&after=%lld",session,(long long)after);
    else n=snprintf(out,cap,"/v1/conversations/%s/messages?limit=5",session);
  }
  return n>=0&&size_t(n)<cap;
}
inline void wipe(void*memory,size_t bytes){volatile uint8_t*p=static_cast<volatile uint8_t*>(memory);while(bytes--)*p++=0;}
}
