#ifndef CARDPUTERZERO_DOCUMENT_H
#define CARDPUTERZERO_DOCUMENT_H

#include <stddef.h>
#include <stdint.h>

int64_t cp0_document_open(void);
int64_t cp0_document_read(int32_t handle, uint64_t offset, uint8_t *buffer,
                          size_t capacity);
int32_t cp0_document_close(int32_t handle);
void cp0_document_destroy(void);

#endif
