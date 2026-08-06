#ifndef CP0_USB_MEDIA_CLIENT_H
#define CP0_USB_MEDIA_CLIENT_H

#include <stdint.h>

#define CP0_USB_MEDIA_ERROR_MAX 160

enum cp0_usb_media_result {
    CP0_USB_MEDIA_OK = 0,
    CP0_USB_MEDIA_FAILED = -1,
    CP0_USB_MEDIA_UNAVAILABLE = -2,
    CP0_USB_MEDIA_INVALID_STATE = -3,
};

enum cp0_usb_media_state {
    CP0_USB_MEDIA_OFF,
    CP0_USB_MEDIA_PREPARING,
    CP0_USB_MEDIA_CONNECTED,
    CP0_USB_MEDIA_IMPORTING,
    CP0_USB_MEDIA_COMPLETE,
    CP0_USB_MEDIA_ERROR,
};

struct cp0_usb_media_status {
    enum cp0_usb_media_state state;
    uint32_t exported_photos;
    uint32_t imported_music;
    uint32_t rejected_music;
    uint64_t capacity_bytes;
};

int cp0_usb_media_get_status(
    struct cp0_usb_media_status *status,
    char error[CP0_USB_MEDIA_ERROR_MAX + 1]);
int cp0_usb_media_start(
    struct cp0_usb_media_status *status,
    char error[CP0_USB_MEDIA_ERROR_MAX + 1]);
int cp0_usb_media_stop(
    struct cp0_usb_media_status *status,
    char error[CP0_USB_MEDIA_ERROR_MAX + 1]);

#endif
