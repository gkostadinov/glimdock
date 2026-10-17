#pragma once
#include "flipper_protocol.h"

typedef struct {
    uint16_t handle;
    uint16_t offset;
    const uint8_t* data;
    uint16_t size;
} GdHciModified;

/* HCI UART/event/vendor envelope. ST attribute-modified code 0x0C01 and
 * ConnectionHandle/AttrHandle/Offset/DataLength layout are verified against
 * stm32wb_copro 133182d5583e998bb263cd947105be4df9c29cb3, used by mntm-012. */
static inline bool gd_hci_attribute_modified(
    const uint8_t* packet, size_t available, GdHciModified* out) {
    if(!packet || !out || available < 13 || packet[0] != 0x04 || packet[1] != 0xFF ||
       packet[2] < 10 || (size_t)packet[2] + 3 > available ||
       gd_read_u16(packet + 3) != 0x0C01) return false;
    uint16_t size = gd_read_u16(packet + 11);
    if(size > packet[2] - 10u) return false;
    out->handle = gd_read_u16(packet + 7);
    out->offset = gd_read_u16(packet + 9);
    out->data = packet + 13;
    out->size = size;
    return true;
}
