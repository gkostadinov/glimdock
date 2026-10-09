#!/usr/bin/env python3
"""Check production pairing validation and NVS save ordering without hardware."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
network = (ROOT / "firmware/src/network.cpp").read_text()
save = network[network.index("bool saveSettings("):network.index("void publishConnection(")]
source = r'''
#include "pairing_policy.h"
#include <iostream>
#include <map>
#include <string>
#include <vector>
#include <cstdlib>
struct Settings {std::string ssid,password,endpoint,token,setupToken,ca;};
struct Preferences {
  static std::map<std::string,std::string> values;
  static std::vector<std::string> writes;
  static std::string failedKey;
  bool begin(const char*,bool){return true;}
  void end(){}
  bool isKey(const char*key){return values.count(key);}
  std::string getString(const char*key){return values[key];}
  size_t putString(const char*key,const std::string&value){writes.push_back(key);if(failedKey==key)return 0;values[key]=value;return value.size()+1;}
};
std::map<std::string,std::string> Preferences::values;
std::vector<std::string> Preferences::writes;
std::string Preferences::failedKey;
''' + save + r'''
unsigned checks=0;
void expect(bool passed,const char*message){if(!passed){std::cerr<<"FAIL: "<<message<<"\n";std::exit(1);}checks++;}
glimdock::PairingSettings view(const Settings&s){return {s.ssid.c_str(),s.password.c_str(),s.endpoint.c_str(),s.token.c_str(),s.setupToken.c_str(),s.ca.c_str()};}
void reset(){Preferences::values.clear();Preferences::writes.clear();Preferences::failedKey.clear();}
int main(){
  Settings complete{"Test network","test-passphrase","http://collector.local:8765/api/v1/snapshot","test-display-token","test-setup-token",""};
  expect(glimdock::pairingSettingsValid(view(complete)),"complete defaults are valid");
  expect(glimdock::shouldPreserveDefaultPairing(true,false,true,view(complete)),"authenticated private defaults are migrated");
  expect(!glimdock::shouldPreserveDefaultPairing(true,false,false,view(complete)),"failed or unauthenticated snapshots never migrate");
  expect(!glimdock::shouldPreserveDefaultPairing(true,true,true,view(complete)),"existing saved pairing never migrates");
  expect(!glimdock::shouldPreserveDefaultPairing(false,false,true,view(complete)),"public builds never migrate private defaults");
  for(const char*endpoint:{"","http://","https://","ftp://collector/snapshot","http://:8765/snapshot","http://collector:/snapshot","http://collector:abc/snapshot","http://collector:65536/snapshot","http://collector:0/snapshot","http://user@collector/snapshot","http://collector /snapshot","http://collector\\path"}){
    auto bad=complete;bad.endpoint=endpoint;expect(!glimdock::pairingSettingsValid(view(bad)),"incomplete or invalid endpoint enters setup");
    expect(!glimdock::shouldPreserveDefaultPairing(true,false,true,view(bad)),"invalid defaults never migrate");
  }
  for(const char*endpoint:{"http://collector.local/api/v1/snapshot","http://127.0.0.1:8765/api/v1/snapshot","http://[::1]:8765/api/v1/snapshot"})expect(glimdock::collectorEndpointValid(endpoint),"valid host and port accepted");
  auto bad=complete;bad.ssid="";expect(!glimdock::pairingSettingsValid(view(bad)),"missing SSID enters setup");
  bad=complete;bad.ssid=std::string(33,'s');expect(!glimdock::pairingSettingsValid(view(bad)),"oversized SSID rejected");
  bad=complete;bad.password="short";expect(!glimdock::pairingSettingsValid(view(bad)),"partial password rejected");
  bad=complete;bad.password="";expect(glimdock::pairingSettingsValid(view(bad)),"open Wi-Fi remains valid");
  bad=complete;bad.token="";expect(!glimdock::pairingSettingsValid(view(bad)),"missing token enters setup");
  bad=complete;bad.token="token\nvalue";expect(!glimdock::pairingSettingsValid(view(bad)),"invalid bearer token rejected");
  bad=complete;bad.setupToken=std::string(193,'t');expect(!glimdock::pairingSettingsValid(view(bad)),"invalid optional setup token rejected");
  bad=complete;bad.endpoint="https://collector/api/v1/snapshot";expect(!glimdock::pairingSettingsValid(view(bad)),"HTTPS without CA enters setup");
  bad.ca="-----BEGIN CERTIFICATE-----\ntest fixture\n-----END CERTIFICATE-----";expect(glimdock::pairingSettingsValid(view(bad)),"HTTPS with CA accepted");
  reset();expect(saveSettings(complete,true),"complete initial settings save verifies");
  expect(Preferences::writes.back()=="ssid","SSID commits the settings set last");
  expect(Preferences::values.size()==6,"all six pairing fields persisted");
  Settings publicSaved{Preferences::values["ssid"],Preferences::values["password"],Preferences::values["endpoint"],Preferences::values["token"],Preferences::values["setup"],Preferences::values["ca"]};
  expect(glimdock::pairingSettingsValid(view(publicSaved)),"public firmware can load the complete saved pairing");
  expect(publicSaved.endpoint==complete.endpoint&&publicSaved.token==complete.token&&publicSaved.password==complete.password&&publicSaved.setupToken==complete.setupToken,"saved pairing preserves all private fields");
  size_t before=Preferences::writes.size();expect(!saveSettings(complete,true)&&Preferences::writes.size()==before,"migration writes only once");
  auto previous=Preferences::values;auto changed=complete;changed.ssid="Another network";changed.token="another-token";
  expect(!saveSettings(changed,true)&&Preferences::values==previous,"migration never changes existing pairing");
  for(const char*key:{"password","endpoint","token","setup","ca"}){reset();Preferences::failedKey=key;expect(!saveSettings(complete,true),"failed settings write does not verify");expect(!Preferences::values.count("ssid"),"partial initial settings are not committed");}
  reset();Preferences::failedKey="ssid";expect(!saveSettings(complete,true)&&!Preferences::values.count("ssid"),"failed commit remains unpaired");
  std::cout<<checks<<" pairing policy and NVS checks passed; no device access\n";
}
'''
with tempfile.TemporaryDirectory(prefix="glimdock-pairing-policy-") as work:
    work = Path(work)
    (work / "test.cpp").write_text(source)
    # The production header intentionally uses compact one-line statements.
    # GCC diagnoses their indentation; retain that warning without treating
    # formatting as a failed policy check. Other warnings remain errors.
    subprocess.run(["c++", "-std=c++17", "-Wall", "-Wextra", "-Werror", "-Wno-error=misleading-indentation", "-I", str(ROOT / "firmware/src"), str(work / "test.cpp"), "-o", str(work / "test")], check=True)
    subprocess.run([str(work / "test")], check=True)
