#include "../../firmware/src/notification_policy.h"
#include <cassert>
#include <cstdio>
int main(){
  NotificationPolicy p;
  uint64_t warning=NotificationPolicy::alertKey("thermal","warning"),critical=NotificationPolicy::alertKey("thermal","critical"),disk=NotificationPolicy::alertKey("disk","warning");
  assert(!p.alertsChanged("lab",&warning,1)); // Existing alerts at startup are a baseline.
  assert(!p.alertsChanged("lab",&warning,1));
  uint64_t both[]={warning,disk};assert(p.alertsChanged("lab",both,2));assert(p.newAlert==1);
  uint64_t reordered[]={disk,warning};assert(!p.alertsChanged("lab",reordered,2));assert(p.newAlert==-1);
  assert(p.alertsChanged("lab",&critical,1)); // Severity escalation is a new event.
  assert(!p.alertsChanged("other",&warning,1));assert(!p.alertsChanged("lab",&critical,1));
  assert(!p.alertsChanged("lab",nullptr,0));assert(p.alertsChanged("lab",&critical,1));
  assert(!p.printerChanged("printer","complete"));assert(!p.printerChanged("printer","printing"));
  assert(!p.printerChanged("printer","paused"));assert(p.printerChanged("printer","complete"));
  assert(!p.printerChanged("printer","complete"));assert(!p.printerChanged("printer","printing"));
  assert(!p.printerChanged("printer","cancelled"));assert(!p.printerChanged("printer","complete"));
  assert(!p.printerChanged("second-printer","printing"));assert(p.printerChanged("second-printer","complete"));
  assert(!p.printerChanged("printer","complete"));
  puts("Notification edges, repeated/reordered snapshots, severity escalation, node isolation and print lifecycle passed.");
}
