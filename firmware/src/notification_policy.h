#pragma once
#include <cstdint>
#include <cstddef>
#include <cstring>
// Per-node edge detection: changing page/order and repeated snapshots stay quiet.
struct NotificationPolicy {
  struct State {char id[64]{};bool alertsSeen=false,printSeen=false,printing=false;uint64_t alerts[24]{};size_t count=0;};
  State nodes[4]{};unsigned replace=0;int newAlert=-1;
  State& state(const char*id){
    for(auto&n:nodes)if(!strcmp(n.id,id))return n;
    for(auto&n:nodes)if(!n.id[0]){std::strncpy(n.id,id,63);return n;}
    State&n=nodes[replace++%4];n=State{};std::strncpy(n.id,id,63);return n;
  }
  static uint64_t alertKey(const char*id,const char*severity){
    uint64_t hash=14695981039346656037ULL;
    for(const char*p=id;*p;p++){hash^=uint8_t(*p);hash*=1099511628211ULL;}
    hash^=0xff;hash*=1099511628211ULL;
    for(const char*p=severity;*p;p++){hash^=uint8_t(*p);hash*=1099511628211ULL;}
    return hash;
  }
  bool alertsChanged(const char*id,const uint64_t*keys,size_t count){
    State&n=state(id);count=count>24?24:count;bool fresh=false;newAlert=-1;
    if(n.alertsSeen)for(size_t i=0;i<count;i++){bool known=false;for(size_t j=0;j<n.count;j++)known|=keys[i]==n.alerts[j];if(!known&&newAlert<0)newAlert=int(i);fresh|=!known;}
    n.alertsSeen=true;n.count=count;for(size_t i=0;i<count;i++)n.alerts[i]=keys[i];return fresh;
  }
  bool printerChanged(const char*id,const char*stage){
    State&n=state(id);bool complete=n.printSeen&&n.printing&&!strcmp(stage,"complete");
    n.printSeen=true;n.printing=!strcmp(stage,"printing")||!strcmp(stage,"paused");return complete;
  }
};
