/* json_parse.c — Read JSON from stdin, parse with cJSON, print formatted output */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "cJSON.h"

/* Read all of stdin into a dynamically allocated buffer */
static char *read_stdin(void) {
    size_t cap = 4096, len = 0;
    char *buf = malloc(cap);
    if (!buf) return NULL;

    size_t n;
    while ((n = fread(buf + len, 1, cap - len, stdin)) > 0) {
        len += n;
        if (len == cap) {
            cap *= 2;
            char *tmp = realloc(buf, cap);
            if (!tmp) { free(buf); return NULL; }
            buf = tmp;
        }
    }
    buf[len] = '\0';
    return buf;
}

/* Print a cJSON value with indentation for readable output */
static void print_value(const cJSON *item, int depth);

static void print_indent(int depth) {
    for (int i = 0; i < depth; i++) printf("  ");
}

static void print_value(const cJSON *item, int depth) {
    if (cJSON_IsNull(item)) {
        printf("null");
    } else if (cJSON_IsFalse(item)) {
        printf("false");
    } else if (cJSON_IsTrue(item)) {
        printf("true");
    } else if (cJSON_IsNumber(item)) {
        double d = item->valuedouble;
        if (d == (int)d && d >= -1e9 && d <= 1e9)
            printf("%d", (int)d);
        else
            printf("%g", d);
    } else if (cJSON_IsString(item)) {
        printf("\"%s\"", item->valuestring);
    } else if (cJSON_IsArray(item)) {
        int size = cJSON_GetArraySize(item);
        if (size == 0) {
            printf("[]");
            return;
        }
        printf("[\n");
        cJSON *child = item->child;
        for (int i = 0; child; child = child->next, i++) {
            print_indent(depth + 1);
            print_value(child, depth + 1);
            if (child->next) printf(",");
            printf("\n");
        }
        print_indent(depth);
        printf("]");
    } else if (cJSON_IsObject(item)) {
        int size = cJSON_GetArraySize(item);
        if (size == 0) {
            printf("{}");
            return;
        }
        printf("{\n");
        cJSON *child = item->child;
        for (int i = 0; child; child = child->next, i++) {
            print_indent(depth + 1);
            printf("\"%s\": ", child->string);
            print_value(child, depth + 1);
            if (child->next) printf(",");
            printf("\n");
        }
        print_indent(depth);
        printf("}");
    }
}

int main(void) {
    char *input = read_stdin();
    if (!input) {
        fprintf(stderr, "Error: failed to read stdin\n");
        return 1;
    }

    cJSON *root = cJSON_Parse(input);
    free(input);

    if (!root) {
        const char *err = cJSON_GetErrorPtr();
        fprintf(stderr, "Parse error near: %.20s\n", err ? err : "(unknown)");
        return 1;
    }

    print_value(root, 0);
    printf("\n");

    cJSON_Delete(root);
    return 0;
}
