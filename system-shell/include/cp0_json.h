#ifndef CP0_JSON_H
#define CP0_JSON_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum cp0_json_type {
    CP0_JSON_OBJECT,
    CP0_JSON_ARRAY,
    CP0_JSON_STRING,
    CP0_JSON_PRIMITIVE,
};

struct cp0_json_token {
    enum cp0_json_type type;
    int start;
    int end;
    int parent;
    unsigned int children;
};

int cp0_json_parse(const char *document, size_t length,
                   struct cp0_json_token *tokens, size_t token_capacity);
int cp0_json_object_get(const char *document,
                        const struct cp0_json_token *tokens, size_t token_count,
                        int object, const char *key);
bool cp0_json_string_equals(const char *document,
                            const struct cp0_json_token *token,
                            const char *expected);
bool cp0_json_copy_string(const char *document,
                          const struct cp0_json_token *token, char *output,
                          size_t output_capacity);
bool cp0_json_get_u64(const char *document,
                      const struct cp0_json_token *token, uint64_t *value);
bool cp0_json_is_null(const char *document,
                      const struct cp0_json_token *token);

#endif
