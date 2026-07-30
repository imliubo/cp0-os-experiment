#include "cp0_json.h"

#include <assert.h>
#include <stdint.h>
#include <string.h>

#define TOKEN_COUNT 64

static int member(const char *document, struct cp0_json_token *tokens,
                  size_t token_count, int object, const char *key)
{
    int value = cp0_json_object_get(document, tokens, token_count, object, key);
    assert(value >= 0);
    return value;
}

int main(void)
{
    static const char response[] =
        "{\"protocol_version\":1,\"request_id\":7,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"pending-permission\","
        "\"prompt\":{\"prompt_id\":42,\"app_name\":\"Hello\\u0020Card\","
        "\"permission\":\"notifications.post\","
        "\"reason\":\"Say \\\"done\\\"\"}}}}";
    struct cp0_json_token tokens[TOKEN_COUNT];
    int count = cp0_json_parse(response, strlen(response), tokens, TOKEN_COUNT);
    assert(count > 0);
    assert(tokens[0].type == CP0_JSON_OBJECT);

    uint64_t value;
    assert(cp0_json_get_u64(
        response, &tokens[member(response, tokens, (size_t)count, 0,
                                 "protocol_version")],
        &value));
    assert(value == 1);
    int outcome = member(response, tokens, (size_t)count, 0, "outcome");
    int data = member(response, tokens, (size_t)count, outcome, "data");
    int prompt = member(response, tokens, (size_t)count, data, "prompt");
    char decoded[64];
    assert(cp0_json_copy_string(
        response,
        &tokens[member(response, tokens, (size_t)count, prompt, "app_name")],
        decoded, sizeof(decoded)));
    assert(strcmp(decoded, "Hello Card") == 0);
    assert(cp0_json_copy_string(
        response,
        &tokens[member(response, tokens, (size_t)count, prompt, "reason")],
        decoded, sizeof(decoded)));
    assert(strcmp(decoded, "Say \"done\"") == 0);

    static const char array[] = "{\"items\":[true,false,{\"id\":9}]}";
    count = cp0_json_parse(array, strlen(array), tokens, TOKEN_COUNT);
    assert(count == 8);
    int items = member(array, tokens, (size_t)count, 0, "items");
    bool boolean;
    assert(cp0_json_get_bool(
        array, &tokens[cp0_json_array_get(tokens, (size_t)count, items, 0)],
        &boolean));
    assert(boolean);
    assert(cp0_json_get_bool(
        array, &tokens[cp0_json_array_get(tokens, (size_t)count, items, 1)],
        &boolean));
    assert(!boolean);
    int object = cp0_json_array_get(tokens, (size_t)count, items, 2);
    assert(object >= 0 && tokens[object].type == CP0_JSON_OBJECT);
    assert(cp0_json_array_get(tokens, (size_t)count, items, 3) < 0);
    assert(!cp0_json_get_bool(
        array, &tokens[member(array, tokens, (size_t)count, object, "id")],
        &boolean));

    static const char with_null[] = "{\"value\":null}";
    count = cp0_json_parse(with_null, strlen(with_null), tokens, TOKEN_COUNT);
    assert(count == 3);
    assert(cp0_json_is_null(
        with_null,
        &tokens[member(with_null, tokens, (size_t)count, 0, "value")]));

    static const char *invalid[] = {
        "", "{", "{\"a\":1", "{\"a\":\"\\x\"}", "{}{}",
        "{\"a\":1,\"b\"}", "[1,2}",
    };
    for (size_t index = 0; index < sizeof(invalid) / sizeof(invalid[0]); index++)
        assert(cp0_json_parse(invalid[index], strlen(invalid[index]), tokens,
                              TOKEN_COUNT) < 0);
    assert(cp0_json_parse(response, strlen(response), tokens, 2) < 0);

    static const char overflow[] = "18446744073709551616";
    count = cp0_json_parse(overflow, strlen(overflow), tokens, TOKEN_COUNT);
    assert(count == 1);
    assert(!cp0_json_get_u64(overflow, &tokens[0], &value));
    return 0;
}
