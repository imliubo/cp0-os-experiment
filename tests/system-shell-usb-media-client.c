#include "cp0_usb_media_client.h"

#include <assert.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

int cp0_usb_media_test_parse(
    const char *response, size_t length, uint64_t request_id,
    struct cp0_usb_media_status *status,
    char error[CP0_USB_MEDIA_ERROR_MAX + 1]);

int main(void)
{
    static const char connected[] =
        "{\"protocol_version\":1,\"request_id\":7,\"outcome\":{"
        "\"status\":\"state\",\"state\":{\"state\":\"connected\","
        "\"exported_photos\":12,\"imported_music\":2,"
        "\"rejected_music\":1,\"capacity_bytes\":536870912}}}";
    static const char unavailable[] =
        "{\"protocol_version\":1,\"request_id\":8,\"outcome\":{"
        "\"status\":\"error\",\"code\":\"unavailable\","
        "\"message\":\"USB controller unavailable\"}}";
    struct cp0_usb_media_status status;
    char error[CP0_USB_MEDIA_ERROR_MAX + 1];

    assert(cp0_usb_media_test_parse(connected, strlen(connected), 7, &status,
                                    error) == CP0_USB_MEDIA_OK);
    assert(status.state == CP0_USB_MEDIA_CONNECTED);
    assert(status.exported_photos == 12 && status.imported_music == 2 &&
           status.rejected_music == 1 && status.capacity_bytes == 536870912);
    assert(cp0_usb_media_test_parse(unavailable, strlen(unavailable), 8,
                                    &status, error) ==
           CP0_USB_MEDIA_UNAVAILABLE);
    assert(strcmp(error, "USB controller unavailable") == 0);
    assert(cp0_usb_media_test_parse(connected, strlen(connected), 6, &status,
                                    error) == CP0_USB_MEDIA_FAILED);
    return 0;
}
