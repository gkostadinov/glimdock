#pragma once
#include <cstddef>
#include <cstring>

namespace glimdock {
struct PairingSettings {const char *ssid,*password,*endpoint,*token,*setupToken,*ca;};
inline bool boundedText(const char*value,size_t minimum,size_t maximum,bool noLineBreaks=false){
  if(!value)return false;size_t length=std::strlen(value);if(length<minimum||length>maximum)return false;
  return !noLineBreaks||(!std::strchr(value,'\r')&&!std::strchr(value,'\n'));
}
inline bool collectorEndpointValid(const char*url){
  if(!boundedText(url,8,240))return false;
  const char*host=!std::strncmp(url,"http://",7)?url+7:!std::strncmp(url,"https://",8)?url+8:nullptr;
  if(!host)return false;
  for(const unsigned char*p=reinterpret_cast<const unsigned char*>(url);*p;p++)if(*p<=32||*p==127||*p=='@'||*p=='\\')return false;
  const char*end=host+std::strcspn(host,"/?#"),*port=nullptr,*hostEnd=end;
  if(host==end)return false;
  if(*host=='['){
    const char*closing=std::strchr(host,']');if(!closing||closing>=end||closing==host+1)return false;
    hostEnd=closing+1;
    if(hostEnd!=end){if(*hostEnd!=':')return false;port=hostEnd+1;}
    for(const char*p=host+1;p<closing;p++)if(!((*p>='0'&&*p<='9')||(*p>='a'&&*p<='f')||(*p>='A'&&*p<='F')||*p==':'||*p=='.'))return false;
  }else{
    for(const char*p=host;p<end;p++)if(*p==':'){if(port)return false;hostEnd=p;port=p+1;}
    if(host==hostEnd)return false;
    for(const unsigned char*p=reinterpret_cast<const unsigned char*>(host);p<reinterpret_cast<const unsigned char*>(hostEnd);p++)if(!((*p>='a'&&*p<='z')||(*p>='A'&&*p<='Z')||(*p>='0'&&*p<='9')||*p=='-'||*p=='.'||*p=='_'||*p>=128))return false;
  }
  if(port){if(port==end)return false;unsigned number=0;for(const char*p=port;p<end;p++){if(*p<'0'||*p>'9')return false;number=number*10+unsigned(*p-'0');if(number>65535)return false;}if(!number)return false;}
  return true;
}
inline bool pairingSettingsValid(const PairingSettings&s){
  if(!boundedText(s.ssid,1,32)||!boundedText(s.password,0,64)||(*s.password&&std::strlen(s.password)<8)||!collectorEndpointValid(s.endpoint)||!boundedText(s.token,1,192,true)||!boundedText(s.setupToken,0,192,true)||!boundedText(s.ca,0,4096))return false;
  return std::strncmp(s.endpoint,"https://",8)||std::strstr(s.ca,"-----BEGIN CERTIFICATE-----");
}
inline bool shouldPreserveDefaultPairing(bool privateDefaults,bool savedSettings,bool authenticatedSnapshot,const PairingSettings&s){
  return privateDefaults&&!savedSettings&&authenticatedSnapshot&&pairingSettingsValid(s);
}
}
