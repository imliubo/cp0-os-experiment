#include "cp0_json.h"

#include <limits.h>
#include <string.h>

#define CP0_JSON_MAX_DEPTH 16U

struct parser {
    const char *document;
    size_t length;
    size_t position;
    struct cp0_json_token *tokens;
    size_t token_capacity;
    size_t token_count;
    int stack[CP0_JSON_MAX_DEPTH];
    size_t depth;
};

static int parent(const struct parser *parser)
{
    return parser->depth == 0 ? -1 : parser->stack[parser->depth - 1];
}

static int allocate_token(struct parser *parser, enum cp0_json_type type,
                          int start, int end)
{
    if (parser->token_count == parser->token_capacity)
        return -1;
    int index = (int)parser->token_count++;
    parser->tokens[index] = (struct cp0_json_token){
        .type = type,
        .start = start,
        .end = end,
        .parent = parent(parser),
    };
    if (parser->tokens[index].parent >= 0)
        parser->tokens[parser->tokens[index].parent].children++;
    return index;
}

static bool hex_digit(char byte)
{
    return (byte >= '0' && byte <= '9') || (byte >= 'a' && byte <= 'f') ||
           (byte >= 'A' && byte <= 'F');
}

static bool parse_string(struct parser *parser)
{
    size_t start = ++parser->position;

    while (parser->position < parser->length) {
        unsigned char byte = (unsigned char)parser->document[parser->position];
        if (byte == '"') {
            if (allocate_token(parser, CP0_JSON_STRING, (int)start,
                               (int)parser->position) < 0)
                return false;
            parser->position++;
            return true;
        }
        if (byte < 0x20U)
            return false;
        if (byte == '\\') {
            parser->position++;
            if (parser->position >= parser->length)
                return false;
            char escape = parser->document[parser->position];
            if (strchr("\"\\/bfnrt", escape) == NULL) {
                if (escape != 'u' || parser->position + 4 >= parser->length)
                    return false;
                for (size_t offset = 1; offset <= 4; offset++) {
                    if (!hex_digit(parser->document[parser->position + offset]))
                        return false;
                }
                parser->position += 4;
            }
        }
        parser->position++;
    }
    return false;
}

static bool primitive_delimiter(char byte)
{
    return byte == ',' || byte == ']' || byte == '}' || byte == ' ' ||
           byte == '\t' || byte == '\r' || byte == '\n';
}

static bool parse_primitive(struct parser *parser)
{
    size_t start = parser->position;
    while (parser->position < parser->length &&
           !primitive_delimiter(parser->document[parser->position])) {
        unsigned char byte = (unsigned char)parser->document[parser->position];
        if (byte < 0x20U || byte >= 0x7fU || byte == ':' || byte == '"' ||
            byte == '[' || byte == '{')
            return false;
        parser->position++;
    }
    if (parser->position == start)
        return false;
    return allocate_token(parser, CP0_JSON_PRIMITIVE, (int)start,
                          (int)parser->position) >= 0;
}

int cp0_json_parse(const char *document, size_t length,
                   struct cp0_json_token *tokens, size_t token_capacity)
{
    struct parser parser = {
        .document = document,
        .length = length,
        .tokens = tokens,
        .token_capacity = token_capacity,
    };
    bool has_root = false;

    if (document == NULL || tokens == NULL || length == 0 ||
        token_capacity == 0 || length > INT_MAX)
        return -1;
    while (parser.position < parser.length) {
        char byte = parser.document[parser.position];
        if (byte == ' ' || byte == '\t' || byte == '\r' || byte == '\n' ||
            byte == ':' || byte == ',') {
            parser.position++;
            continue;
        }
        if (parser.depth == 0 && has_root)
            return -1;
        if (byte == '{' || byte == '[') {
            if (parser.depth == CP0_JSON_MAX_DEPTH)
                return -1;
            int token = allocate_token(
                &parser, byte == '{' ? CP0_JSON_OBJECT : CP0_JSON_ARRAY,
                (int)parser.position, -1);
            if (token < 0)
                return -1;
            parser.stack[parser.depth++] = token;
            parser.position++;
            has_root = true;
            continue;
        }
        if (byte == '}' || byte == ']') {
            if (parser.depth == 0)
                return -1;
            int token = parser.stack[parser.depth - 1];
            enum cp0_json_type expected =
                byte == '}' ? CP0_JSON_OBJECT : CP0_JSON_ARRAY;
            if (parser.tokens[token].type != expected ||
                (expected == CP0_JSON_OBJECT &&
                 parser.tokens[token].children % 2U != 0U))
                return -1;
            parser.tokens[token].end = (int)++parser.position;
            parser.depth--;
            continue;
        }
        if (byte == '"') {
            if (!parse_string(&parser))
                return -1;
        } else if (!parse_primitive(&parser)) {
            return -1;
        }
        has_root = true;
    }
    if (!has_root || parser.depth != 0)
        return -1;
    return (int)parser.token_count;
}

static int after_subtree(const struct cp0_json_token *tokens,
                         size_t token_count, int token)
{
    int end = tokens[token].end;
    int index = token + 1;
    while ((size_t)index < token_count && tokens[index].start < end)
        index++;
    return index;
}

int cp0_json_object_get(const char *document,
                        const struct cp0_json_token *tokens, size_t token_count,
                        int object, const char *key)
{
    if (document == NULL || tokens == NULL || key == NULL || object < 0 ||
        (size_t)object >= token_count ||
        tokens[object].type != CP0_JSON_OBJECT)
        return -1;
    int index = object + 1;
    while ((size_t)index < token_count &&
           tokens[index].start < tokens[object].end) {
        if (tokens[index].parent != object ||
            tokens[index].type != CP0_JSON_STRING ||
            (size_t)(index + 1) >= token_count ||
            tokens[index + 1].parent != object)
            return -1;
        int value = index + 1;
        if (cp0_json_string_equals(document, &tokens[index], key))
            return value;
        index = after_subtree(tokens, token_count, value);
    }
    return -1;
}

int cp0_json_array_get(const struct cp0_json_token *tokens, size_t token_count,
                       int array, unsigned int item)
{
    unsigned int current = 0;
    int index;

    if (tokens == NULL || array < 0 || (size_t)array >= token_count ||
        tokens[array].type != CP0_JSON_ARRAY)
        return -1;
    index = array + 1;
    while ((size_t)index < token_count &&
           tokens[index].start < tokens[array].end) {
        if (tokens[index].parent != array)
            return -1;
        if (current++ == item)
            return index;
        index = after_subtree(tokens, token_count, index);
    }
    return -1;
}

bool cp0_json_string_equals(const char *document,
                            const struct cp0_json_token *token,
                            const char *expected)
{
    if (document == NULL || token == NULL || expected == NULL ||
        token->type != CP0_JSON_STRING || token->start < 0 ||
        token->end < token->start)
        return false;
    size_t length = (size_t)(token->end - token->start);
    return strlen(expected) == length &&
           memcmp(document + token->start, expected, length) == 0;
}

static int hex_value(char byte)
{
    if (byte >= '0' && byte <= '9')
        return byte - '0';
    if (byte >= 'a' && byte <= 'f')
        return byte - 'a' + 10;
    return byte - 'A' + 10;
}

static bool append_utf8(char *output, size_t capacity, size_t *offset,
                        uint32_t codepoint)
{
    unsigned int bytes;
    if (codepoint <= 0x7fU)
        bytes = 1;
    else if (codepoint <= 0x7ffU)
        bytes = 2;
    else if (codepoint <= 0xffffU &&
             !(codepoint >= 0xd800U && codepoint <= 0xdfffU))
        bytes = 3;
    else
        return false;
    if (*offset + bytes >= capacity)
        return false;
    if (bytes == 1) {
        output[(*offset)++] = (char)codepoint;
    } else if (bytes == 2) {
        output[(*offset)++] = (char)(0xc0U | (codepoint >> 6U));
        output[(*offset)++] = (char)(0x80U | (codepoint & 0x3fU));
    } else {
        output[(*offset)++] = (char)(0xe0U | (codepoint >> 12U));
        output[(*offset)++] =
            (char)(0x80U | ((codepoint >> 6U) & 0x3fU));
        output[(*offset)++] = (char)(0x80U | (codepoint & 0x3fU));
    }
    return true;
}

bool cp0_json_copy_string(const char *document,
                          const struct cp0_json_token *token, char *output,
                          size_t output_capacity)
{
    size_t offset = 0;
    if (document == NULL || token == NULL || output == NULL ||
        output_capacity == 0 || token->type != CP0_JSON_STRING ||
        token->start < 0 || token->end < token->start)
        return false;
    for (int index = token->start; index < token->end; index++) {
        unsigned char byte = (unsigned char)document[index];
        if (byte != '\\') {
            if (offset + 1 >= output_capacity)
                return false;
            output[offset++] = (char)byte;
            continue;
        }
        if (++index >= token->end)
            return false;
        char escape = document[index];
        if (escape == 'u') {
            if (index + 4 >= token->end)
                return false;
            uint32_t codepoint = 0;
            for (int digit = 0; digit < 4; digit++)
                codepoint = (codepoint << 4U) |
                            (uint32_t)hex_value(document[++index]);
            if (!append_utf8(output, output_capacity, &offset, codepoint))
                return false;
        } else {
            char decoded;
            switch (escape) {
            case '"': case '\\': case '/': decoded = escape; break;
            case 'b': decoded = '\b'; break;
            case 'f': decoded = '\f'; break;
            case 'n': decoded = '\n'; break;
            case 'r': decoded = '\r'; break;
            case 't': decoded = '\t'; break;
            default: return false;
            }
            if (offset + 1 >= output_capacity)
                return false;
            output[offset++] = decoded;
        }
    }
    output[offset] = '\0';
    return true;
}

bool cp0_json_get_u64(const char *document,
                      const struct cp0_json_token *token, uint64_t *value)
{
    uint64_t result = 0;
    if (document == NULL || token == NULL || value == NULL ||
        token->type != CP0_JSON_PRIMITIVE || token->start < 0 ||
        token->end <= token->start)
        return false;
    for (int index = token->start; index < token->end; index++) {
        char byte = document[index];
        if (byte < '0' || byte > '9' ||
            result > (UINT64_MAX - (uint64_t)(byte - '0')) / 10U)
            return false;
        result = result * 10U + (uint64_t)(byte - '0');
    }
    *value = result;
    return true;
}

bool cp0_json_get_bool(const char *document,
                       const struct cp0_json_token *token, bool *value)
{
    if (document == NULL || token == NULL || value == NULL ||
        token->type != CP0_JSON_PRIMITIVE)
        return false;
    if (token->end - token->start == 4 &&
        memcmp(document + token->start, "true", 4) == 0) {
        *value = true;
        return true;
    }
    if (token->end - token->start == 5 &&
        memcmp(document + token->start, "false", 5) == 0) {
        *value = false;
        return true;
    }
    return false;
}

bool cp0_json_is_null(const char *document,
                      const struct cp0_json_token *token)
{
    if (document == NULL || token == NULL || token->type != CP0_JSON_PRIMITIVE)
        return false;
    return token->end - token->start == 4 &&
           memcmp(document + token->start, "null", 4) == 0;
}
