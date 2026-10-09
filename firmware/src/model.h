#pragma once
#include <Arduino.h>
#include <cmath>
#include <limits>
#include <new>
#include <cstring>
constexpr size_t MAX_GUESTS=48, MAX_SENSORS=64, MAX_STORAGE=16, MAX_DISKS=16, MAX_ALERTS=24, MAX_CORES=64, MAX_SOURCES=16, MAX_FAULTS=16, MAX_GPUS=8, MAX_NODES=4, MAX_HEATERS=8, MAX_PRINTER_TEMPS=16, MAX_INTERFACES=16;
constexpr size_t MAX_JSON=48*1024;
constexpr size_t MAX_CONFIG_NODES=16, MAX_CONFIG_JSON=16*1024;
constexpr float UNKNOWN = std::numeric_limits<float>::quiet_NaN();
struct Guest { int id=0; char name[64]{},type[8]{},status[16]{},memoryBasis[20]{"proxmox"},memError[128]{}; float cpu=UNKNOWN; double used=NAN,total=NAN,uptime=NAN,rx=NAN,tx=NAN,read=NAN,write=NAN,available=NAN,cache=NAN,noncache=NAN,assigned=NAN,hostMem=NAN,pveUsed=NAN,pveTotal=NAN,memUpdated=NAN,memAge=NAN; };
struct Gpu {char id[64]{},name[96]{},vendor[32]{},kind[16]{},owner[32]{},driver[48]{},status[24]{},error[128]{},utilizationKind[24]{"gpu"};float utilization=UNKNOWN,temp=UNKNOWN,power=UNKNOWN,graphicsMhz=UNKNOWN,memoryMhz=UNKNOWN,fan=UNKNOWN;double memUsed=NAN,memTotal=NAN,updated=NAN,age=NAN;};
struct Pool { char id[48]{},name[64]{},status[20]{}; double used=NAN,total=NAN; };
struct Disk { char name[48]{},model[64]{},health[20]{},status[20]{},zfsStatus[24]{}; float temp=UNKNOWN; double read=NAN,write=NAN,wear=NAN,mediaErrors=NAN,powerHours=NAN,spare=NAN,reallocated=NAN,pending=NAN,zfsReadErrors=NAN,zfsWriteErrors=NAN,zfsChecksumErrors=NAN; };
struct Sensor { char id[96]{},name[64]{},chip[48]{},kind[16]{},unit[12]{}; float value=UNKNOWN,crit=UNKNOWN,high=UNKNOWN; };
struct Alert { char id[64]{},severity[16]{},message[160]{}; };
struct Fault {char id[64]{},kind[16]{},message[160]{};double timestamp=NAN;};
struct Source { char name[32]{},error[96]{}; bool ok=false,enabled=true; double updated=NAN,age=NAN; };
struct DeviceInterface {char name[64]{},status[16]{};double speed=NAN,rx=NAN,tx=NAN,errorsIn=NAN,errorsOut=NAN,dropsIn=NAN,dropsOut=NAN;};
struct NodeSummary {bool available=false;float cpu=UNKNOWN,memory=UNKNOWN,temp=UNKNOWN,progress=UNKNOWN;char printState[24]{},status[24]{};int32_t sensors=-1,guests=-1;double generated=NAN,age=NAN,ttl=15;};
struct NodeDescriptor {char id[64]{},type[16]{},platform[16]{},name[64]{},address[96]{},status[24]{};NodeSummary summary;};
inline bool localNodeType(const char*type){return !strcmp(type,"local-proxmox")||!strcmp(type,"local-server");}
inline bool agentNodeType(const char*type){return !strcmp(type,"server-agent");}
inline bool deviceNodeType(const char*type){return !strcmp(type,"server")||!strcmp(type,"local-server")||!strcmp(type,"server-feed")||agentNodeType(type);}
inline const char*platformName(const char*platform){return !strcmp(platform,"linux")?"Linux":!strcmp(platform,"macos")?"macOS":!strcmp(platform,"windows")?"Windows":!strcmp(platform,"router")?"Router":!strcmp(platform,"other")?"Other device":"Server / device";}
inline const char*nodeTypeName(const NodeDescriptor&node){return !strcmp(node.type,"klipper")?"Klipper":!strcmp(node.type,"proxmox")||!strcmp(node.type,"proxmox-feed")||!strcmp(node.type,"local-proxmox")?"Proxmox":platformName(node.platform);}
struct Heater {char name[64]{};float temp=UNKNOWN,target=UNKNOWN,duty=UNKNOWN;};
struct PrinterTemperature {char name[64]{};float temp=UNKNOWN;};
struct Printer {bool present=false;char id[64]{},name[64]{},host[96]{},hostName[64]{},klippyState[24]{},state[24]{},message[160]{},filename[192]{},progressBasis[48]{},etaBasis[64]{},error[128]{};float progress=UNKNOWN,fan=UNKNOWN;double printDuration=NAN,totalDuration=NAN,currentLayer=NAN,totalLayers=NAN,filament=NAN,slicerTime=NAN,remaining=NAN,eta=NAN,updated=NAN,age=NAN,ttl=15;int8_t filamentDetected=-1;uint8_t nHeaters=0,nTemps=0;Heater heaters[MAX_HEATERS];PrinterTemperature temperatures[MAX_PRINTER_TEMPS];};
inline bool validNodeId(const char*id){if(!id||!id[0])return false;size_t n=0;for(;id[n];n++){unsigned char c=id[n];if(n>=63||!((c>='a'&&c<='z')||(c>='A'&&c<='Z')||(c>='0'&&c<='9')||c==':'||c=='.'||c=='_'||c=='-'))return false;}return true;}
inline bool encodeNodeId(const char*id,char*out,size_t cap){if(!validNodeId(id)||!out)return false;size_t n=0;const char*hex="0123456789ABCDEF";for(size_t i=0;id[i];i++){unsigned char c=id[i];bool plain=(c>='a'&&c<='z')||(c>='A'&&c<='Z')||(c>='0'&&c<='9')||c=='.'||c=='_'||c=='-';size_t need=plain?1:3;if(n+need>=cap)return false;if(plain)out[n++]=c;else{out[n++]='%';out[n++]=hex[c>>4];out[n++]=hex[c&15];}}out[n]=0;return true;}
struct Snapshot {
  bool valid=false,demo=false; uint64_t sequence=0; double generated=NAN; uint32_t received=0;
  char host[64]{},ip[48]{}; double uptime=NAN,memUsed=NAN,memTotal=NAN,swapUsed=NAN,swapTotal=NAN,arc=NAN,rx=NAN,tx=NAN,read=NAN,write=NAN;
  float cpu=UNKNOWN,iowait=UNKNOWN,temp=UNKNOWN,watts=UNKNOWN,mhz=UNKNOWN,busyMhz=UNKNOWN,load[3]{UNKNOWN,UNKNOWN,UNKNOWN};
  float cores[MAX_CORES]{},cstates[10]{}; char cstateNames[10][12]{};
  uint8_t nGuests=0,nPools=0,nDisks=0,nSensors=0,nAlerts=0,nCores=0,nCstates=0,nSources=0,nFaults=0,nGpus=0;
  bool truncated=false;int faultLookbackDays=7;double segfaults24h=NAN,faultEvents24h=NAN,lastFaultAt=NAN;
  NodeDescriptor node,nodes[MAX_NODES];uint8_t nNodes=0;Printer printer;
  char osRelease[97]{},osVersion[97]{},architecture[49]{},uptimeScope[24]{};uint16_t cpuCount=0;double batteryPercent=NAN,batteryRemaining=NAN;int8_t batteryPlugged=-1;uint8_t nInterfaces=0;DeviceInterface interfaces[MAX_INTERFACES];
  Guest guests[MAX_GUESTS]; Pool pools[MAX_STORAGE]; Disk disks[MAX_DISKS]; Sensor sensors[MAX_SENSORS]; Alert alerts[MAX_ALERTS]; Source sources[MAX_SOURCES];Fault faults[MAX_FAULTS];Gpu gpus[MAX_GPUS];
};
inline void clearForNode(Snapshot&s,const NodeDescriptor&node){NodeDescriptor chosen=node,registry[MAX_NODES];uint8_t count=s.nNodes;memcpy(registry,s.nodes,sizeof(registry));new(&s)Snapshot{};s.node=chosen;s.nNodes=count;memcpy(s.nodes,registry,sizeof(registry));}
inline int nodeIndexForId(const Snapshot&s,const char*id){if(!validNodeId(id))return -1;for(int i=0;i<s.nNodes;i++)if(!strcmp(s.nodes[i].id,id))return i;return -1;}
inline bool nodeReplyMatches(const Snapshot&s,const char*requested,uint32_t capturedGeneration,uint32_t currentGeneration){return capturedGeneration==currentGeneration&&(!requested[0]||!strcmp(requested,s.node.id));}
struct NetworkState { bool configured=false,wifi=false,setup=false,loading=false; char message[96]{},apPassword[16]{},ip[48]{}; uint32_t revision=0; };
struct ConnectionSettings {char ssid[33]{},endpoint[241]{};bool available=false,passwordSaved=false,displayTokenSaved=false,setupTokenSaved=false,caSaved=false,busy=false,saved=false;char message[128]{};uint32_t revision=0;};
struct ConnectionUpdate {char ssid[33]{},password[65]{},endpoint[241]{},displayToken[193]{},setupToken[193]{};bool clearPassword=false;};
struct ConfigNode {char id[64]{},type[20]{},platform[16]{},origin[16]{},name[64]{},url[241]{};double poll=5,timeout=3,ttl=15;bool hasSecret=false,enabled=true,registered=false;};
struct NodeConfigDraft {ConfigNode node;char secret[257]{};bool clearSecret=false;};
struct NodeConfiguration {bool available=false,busy=false,applying=false,saved=false;char version[65]{},message[160]{},localId[64]{},localName[64]{},localAddress[96]{},localType[16]{"server"},localPlatform[16]{};bool localEnabled=false;uint8_t count=0;ConfigNode nodes[MAX_CONFIG_NODES];uint32_t revision=0;};
inline uint8_t activeConfiguredNodes(const NodeConfiguration&config){uint8_t count=0;for(int i=0;i<config.count;i++)if(config.nodes[i].enabled)count++;return count;}
inline NodeDescriptor configuredRegistry(Snapshot&snapshot,const NodeConfiguration&config,const char*selectedId){
  NodeDescriptor selected;snapshot.nNodes=0;
  for(int i=0;i<config.count&&snapshot.nNodes<MAX_NODES;i++){
    const auto&node=config.nodes[i];if(!node.enabled)continue;
    auto&descriptor=snapshot.nodes[snapshot.nNodes++];descriptor=NodeDescriptor{};
    strlcpy(descriptor.id,node.id,sizeof(descriptor.id));
    strlcpy(descriptor.type,!strcmp(node.type,"klipper")?"klipper":deviceNodeType(node.type)?"server":"proxmox",sizeof(descriptor.type));
    strlcpy(descriptor.platform,node.platform,sizeof(descriptor.platform));strlcpy(descriptor.name,node.name,sizeof(descriptor.name));
    strlcpy(descriptor.address,localNodeType(node.type)?config.localAddress:node.url,sizeof(descriptor.address));
    strlcpy(descriptor.status,"applying",sizeof(descriptor.status));if(!strcmp(node.id,selectedId))selected=descriptor;
  }
  return selected;
}
inline bool settleNodeConfiguration(NodeConfiguration&config){if(config.busy||!config.applying)return false;config.applying=false;strlcpy(config.message,"Node settings saved. Monitoring resumed.",sizeof(config.message));config.revision++;return true;}
bool readConnectionSettings(ConnectionSettings&out);
bool requestConnectionUpdate(const ConnectionUpdate&update);
bool readNodeConfiguration(NodeConfiguration&out);
bool requestNodeConfiguration();
bool requestNodeUpsert(const NodeConfigDraft&draft,const char*version);
bool requestNodeDelete(const char*id,const char*version);
extern Snapshot *publishedSnapshot;
extern SemaphoreHandle_t dataMutex;
extern NetworkState networkState;
extern volatile bool requestSetup;
bool requestNodeSelection(const char*id);
void startNetwork();
bool readSnapshot(Snapshot &out,NetworkState &state);
void makeDemo(Snapshot &out);
