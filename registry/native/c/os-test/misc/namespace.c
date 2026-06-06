/*
 * Copyright (c) 2025 Jonas 'Sortie' Termansen.
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 *
 * namespace.c
 * Parse preprocessed C headers and check for namespace pollution per the API.
 */

#include <sys/stat.h>

#include <ctype.h>
#include <errno.h>
#include <libgen.h>
#include <limits.h>
#include <regex.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "../misc/errors.h"

// TODO: Workaround for broken redox getopt weak symbols.
#ifdef __redox__
char* optarg;
int optind;
#endif

enum type
{
	TYPE_DEFINITION,
	TYPE_FUNCTION,
	TYPE_GENERIC,
	TYPE_EXTERNAL,
	TYPE_SYMBOLIC_CONSTANT,
	TYPE_ENUMERATION,
	TYPE_ENUMERATION_MEMBER,
	TYPE_UNION,
	TYPE_UNION_MEMBER,
	TYPE_STRUCTURE,
	TYPE_STRUCTURE_MEMBER,
	TYPE_TYPE,
	TYPE_EXPRESSION,
	TYPE_INCLUDE,
	TYPE_NAMESPACE,
	TYPE_COUNT,
	TYPE_FIRST = TYPE_DEFINITION,
};

const char* type_names[] =
{
        [TYPE_DEFINITION] = "define",
        [TYPE_FUNCTION] = "function",
        [TYPE_GENERIC] = "generic",
        [TYPE_EXTERNAL] = "external",
        [TYPE_SYMBOLIC_CONSTANT] = "symbolic_constant",
        [TYPE_ENUMERATION] = "enum",
        [TYPE_ENUMERATION_MEMBER] = "enum_member",
        [TYPE_UNION] = "union",
        [TYPE_UNION_MEMBER] = "union_member",
        [TYPE_STRUCTURE] = "struct",
        [TYPE_STRUCTURE_MEMBER] = "struct_member",
        [TYPE_TYPE] = "typedef",
        [TYPE_EXPRESSION] = "expression",
        [TYPE_INCLUDE] = "include",
        [TYPE_NAMESPACE] = "namespace",
};

struct declaration
{
	int type_mask;
	char* name;
	char* sig;
	char* parent;
	char* options;
	bool optional;
	bool incomplete;
};

#define REQUIRED_TYPE(type) (1 << (type))
#define OPTIONAL_TYPE(type) (1 << ((type) + TYPE_COUNT))

static struct declaration** declarations = NULL;
static size_t declarations_used = 0;
static bool polluted = false;
static bool xsi = false;

// Avoid asprintf since it's not portable to POSIX 2008.
static int format_string(char** restrict result_ptr,
                         const char* restrict format,
                         ...)
{
	va_list list, list2;
	va_start(list, format);
	va_copy(list2, list);
	int length = vsnprintf(NULL, 0, format, list);
	va_end(list);
	char* buffer = malloc(length + 1);
	*result_ptr = buffer;
	if ( !buffer )
	{
		va_end(list2);
		return -1;
	}
	length = vsnprintf(buffer, length + 1, format, list2);
	va_end(list2);
	return length;
}

static char* join_paths(const char* a, const char* b)
{
	size_t a_len = strlen(a);
	bool has_slash = (a_len && a[a_len-1] == '/') || b[0] == '/';
	char* result;
	if ( (has_slash && format_string(&result, "%s%s", a, b) < 0) ||
	     (!has_slash && format_string(&result, "%s/%s", a, b) < 0) )
		return NULL;
	return result;
}

static int mkdir_p(const char* path, mode_t mode)
{
	int saved_errno = errno;
	if ( !mkdir(path, mode) )
		return 0;
	if ( errno == ENOENT )
	{
		char* prev = strdup(path);
		if ( !prev )
			return -1;
		// Much shame, dirname is still not thread safe on many systems.
		const char* dir = dirname(prev);
		char* parent = strdup(dir);
		free(prev);
		if ( !parent )
			return -1;
		int status = mkdir_p(parent, mode | 0500);
		free(parent);
		if ( status < 0 )
			return -1;
		errno = saved_errno;
		if ( !mkdir(path, mode) )
			return 0;
	}
	if ( errno == EEXIST )
		return errno = saved_errno, 0;
	return -1;
}

static void mkdir_parent_of(const char* path, mode_t mode)
{
	char* copy = strdup(path);
	if ( !copy )
		err(1, "malloc");
	const char* dir = dirname(copy);
	char* parent = strdup(dir);
	if ( !parent )
		err(1, "malloc");
	if ( mkdir_p(parent, mode) < 0 )
		err(1, "%s", parent);
	free(copy);
	free(parent);
}

static char* read_text_file(const char* path)
{
	FILE* fp = fopen(path, "r");
	if ( !fp )
		return NULL;
	char* result = NULL;
	size_t size = 0;
	if ( getdelim(&result, &size, '\0', fp) < 0 )
	{
		free(result);
		if ( !errno )
			return strdup("");
		return NULL;
	}
	fclose(fp);
	return result;
}

static bool array_add(char*** array_ptr,
                      size_t* used_ptr,
                      size_t* length_ptr,
                      char* value)
{
	char** array = *array_ptr;

	if ( *used_ptr == *length_ptr )
	{
		size_t length = *length_ptr;
		if ( !length )
			length = 4;
		// For portability, don't require POSIX 2024 reallocarray.
		if ( SIZE_MAX / (2 * sizeof(char*)) <= length )
			return errno = ENOMEM, false;
		size_t new_size = length * 2 * sizeof(char*);
		char** new_array = realloc(array, new_size);
		if ( !new_array )
			return false;
		array = new_array;
		*array_ptr = array;
		*length_ptr = length * 2;
	}

	array[(*used_ptr)++] = value;

	return true;
}

static bool is_identifier(char c)
{
	return ('a' <= c && c <= 'z') || ('A' <= c && c <= 'Z') ||
	       ('0' <= c && c <= '9') || c == '_';
}

static bool next_is_token(const char* input, const char* token)
{
	return !strncmp(input, token, strlen(token)) &&
	       (!input[strlen(token)] ||
	        isspace((unsigned char) input[strlen(token)]) ||
	        (is_identifier(input[strlen(token)])) !=
	         is_identifier(token[0]));
}

static struct declaration* parse_declaration(const char* input)
{
	struct declaration* declaration = calloc(1, sizeof(struct declaration));
	if ( !declaration )
		abort();
	while ( isspace((unsigned char) input[0]) )
		input++;
	if ( input[0] == '[' )
	{
		input++;
		size_t length = 0;
		while ( input[length] && input[length] != ']' )
			length++;
		if ( input[length] != ']' )
			abort();
		if ( !(declaration->options = strndup(input, length)) )
			abort();
		input += length + 1;
		while ( isspace((unsigned char) input[0]) )
			input++;
	}
	if ( next_is_token(input, "optional") )
	{
		declaration->optional = true;
		input += strlen("optional");
		while ( isspace((unsigned char) input[0]) )
			input++;
	}
	if ( next_is_token(input, "incomplete") )
	{
		declaration->incomplete = true;
		input += strlen("incomplete");
		while ( isspace((unsigned char) input[0]) )
			input++;
	}
	if ( next_is_token(input, "parent") )
	{
		input += strlen("parent");
		while ( isspace((unsigned char) input[0]) )
			input++;
		size_t length = 0;
		if ( next_is_token(input, "struct") )
			length += strlen("struct");
		else if ( next_is_token(input, "union") )
			length += strlen("union");
		while ( isspace((unsigned char) input[length]) )
			length++;
		while ( input[length] && !isspace((unsigned char) input[length]) )
			length++;
		if ( !length )
			abort();
		if ( !(declaration->parent = strndup(input, length)) )
			abort();
		input += length;
		while ( isspace((unsigned char) input[0]) )
			input++;
	}
	for ( enum type type = TYPE_FIRST; type < TYPE_COUNT; type++ )
	{
		if ( !strncmp(input, "maybe_", strlen("maybe_")) &&
		     next_is_token(input + strlen("maybe_"), type_names[type]) )
		{
			input += strlen("maybe_") + strlen(type_names[type]);
			declaration->type_mask |= OPTIONAL_TYPE(type);
		}
		else if ( next_is_token(input, type_names[type]) )
		{
			input += strlen(type_names[type]);
			declaration->type_mask |= REQUIRED_TYPE(type);
		}
		while ( isspace((unsigned char) input[0]) )
			input++;
	}
	if ( !declaration->type_mask )
		abort();
	size_t length = 0;
	while ( input[length] && !isspace((unsigned char) input[length]) &&
	        input[length] != ';' && input[length] != ':' )
		length++;
	if ( !length )
		abort();
	if ( !(declaration->name = strndup(input, length)) )
		abort();
	input += length;
	while ( isspace((unsigned char) input[0]) )
		input++;
	if ( input[0] == ':' )
	{
		input++;
		while ( isspace((unsigned char) input[0]) )
			input++;
		length = 0;
		while ( input[length] && input[length] != ';' )
			length++;
		if ( input[length] != ';' )
			abort();
		if ( !(declaration->sig = strndup(input, length)) )
			abort();
		input += length;
	}
	else if ( !(declaration->sig = strdup(declaration->name)) )
		abort();
	if ( strcmp(input, ";") != 0 )
		abort();
	return declaration;
}

static bool type_check(enum type type, int type_mask)
{
	if ( type == TYPE_COUNT )
		return true;
	if ( (type_mask & REQUIRED_TYPE(TYPE_NAMESPACE)) )
		return true;
	if ( (type_mask & REQUIRED_TYPE(TYPE_DEFINITION)) &&
	     (type == TYPE_EXTERNAL) )
		return true;
	if ( (type_mask & REQUIRED_TYPE(TYPE_SYMBOLIC_CONSTANT)) &&
	     (type == TYPE_DEFINITION || type == TYPE_ENUMERATION_MEMBER) )
		return true;
	if ( (type_mask & REQUIRED_TYPE(TYPE_EXPRESSION)) &&
	     (type == TYPE_DEFINITION || type == TYPE_EXTERNAL ||
	      type == TYPE_ENUMERATION_MEMBER) )
		return true;
	return type_mask & (REQUIRED_TYPE(type) | OPTIONAL_TYPE(type));
}

static void found_name(const char* name, enum type type)
{
	// TODO: sort and bsearch for fast lookup
	bool reserved = (name[0] == '_' && name[1] == '_') ||
	                (name[0] == '_' && 'A' <= name[1] && name[1] <= 'Z') ||
	                (name[0] == '_');
	bool found = false;
	int wrong_mask = 0;
	struct declaration* wrong_declaration = NULL;
	for ( size_t i = 0; i < declarations_used; i++ )
	{
		struct declaration* declaration = declarations[i];
		if ( declaration->options &&
		     (!strcmp(declaration->options, "XSI") ||
		      strstr(declaration->options, "XSI ")) &&
		     !xsi )
			continue;
		if ( declaration->type_mask == REQUIRED_TYPE(TYPE_INCLUDE) )
			continue;
		if ( declaration->type_mask == REQUIRED_TYPE(TYPE_NAMESPACE) )
		{
			regex_t re;
			if ( regcomp(&re, declaration->sig, REG_EXTENDED) )
				err(1, "regcomp: %s", declaration->sig);
			regmatch_t match;
			bool result = !regexec(&re, name, 1, &match, 0);
			regfree(&re);
			if ( !result )
				continue;
		}
		else if ( strcmp(declaration->name, name) != 0 )
			continue;
		if ( !type_check(type, declaration->type_mask) )
		{
			wrong_mask |= declaration->type_mask;
			continue;
		}
		found = true;
		break;
	}
	if ( found )
	{
		//printf("found %s\n", name);
	}
	else if ( reserved )
	{
		//printf("reserved %s\n", name);
	}
	else
	{
		if ( wrong_mask )
		{
			printf("wrong type: %s: expected", type_names[type]);
			const char* or = "";
			for ( enum type t = TYPE_FIRST; t < TYPE_COUNT; t++ )
			{
				if ( wrong_mask & REQUIRED_TYPE(t) )
				{
					printf(" %s%s", or, type_names[t]);
					or = "or ";
				}
				if ( wrong_mask & OPTIONAL_TYPE(t) )
				{
					printf(" %smaybe %s", or, type_names[t]);
					or = "or ";
				}
			}
			printf(": %s\n", name);
		}
		else
			printf("pollution: %s\n", name);
		polluted = true;
	}
}

void output_collapsed_space(const char* string, size_t length, FILE* fp)
{
	size_t i = 0;
	while ( i < length )
	{
		unsigned char uc = string[i++];
		if ( isspace(uc) )
			uc = ' ';
		fputc(uc, fp);
		if ( uc == ' ' )
		{
			while ( string[i] && isspace((unsigned char) string[i]) )
				i++;
		}
	}
}

static bool parse_space(const char** input_ptr)
{
	if ( !isspace((unsigned char) **input_ptr) )
		return false;
	while ( isspace((unsigned char) **input_ptr) )
		(*input_ptr)++;
	return true;
}

static bool parse_char(const char** input_ptr, char c)
{
	if ( **input_ptr != c )
		return false;
	(*input_ptr)++;
	parse_space(input_ptr);
	return true;
}

static bool parse_token(const char** input_ptr, const char* token)
{
	if ( !next_is_token(*input_ptr, token) )
		return false;
	*input_ptr = *input_ptr + strlen(token);
	while ( isspace((unsigned char) **input_ptr) )
		(*input_ptr)++;
	return true;
}

static bool parse_new_attribute(const char** input_ptr)
{
	if ( (*input_ptr)[0] != '[' || (*input_ptr)[1] != '[' )
		return false;
	(*input_ptr) += 2;
	const char* input = *input_ptr;
	size_t i = 0;
	i++;
	size_t depth = 2;
	for ( ; input[i] && depth; i++ )
	{
		if ( input[i] == '[' )
			depth++;
		else if ( input[i] == ']' )
			depth--;
	}
	*input_ptr = input + i;
	parse_space(input_ptr);
	return !depth;
}

static bool parse_attribute(const char** input_ptr)
{
	if ( !parse_token(input_ptr, "__attribute__") )
		return false;
	const char* input = *input_ptr;
	size_t i = 0;
	while ( input[i] && isspace((unsigned char) input[i]) )
		i++;
	if ( input[i] != '(' )
		return false;
	i++;
	size_t depth = 1;
	for ( ; input[i] && depth; i++ )
	{
		if ( input[i] == '(' )
			depth++;
		else if ( input[i] == ')' )
			depth--;
	}
	*input_ptr = input + i;
	parse_space(input_ptr);
	return !depth;
}

static bool parse_asm(const char** input_ptr)
{
	if ( !parse_token(input_ptr, "__asm") &&
	     !parse_token(input_ptr, "__asm__") )
		return false;
	const char* input = *input_ptr;
	size_t i = 0;
	while ( input[i] && isspace((unsigned char) input[i]) )
		i++;
	if ( input[i] != '(' )
		return false;
	i++;
	size_t depth = 1;
	for ( ; input[i] && depth; i++ )
	{
		if ( input[i] == '(' )
			depth++;
		else if ( input[i] == ')' )
			depth--;
	}
	*input_ptr = input + i;
	parse_space(input_ptr);
	return !depth;
}

static bool parse_typeof(const char** input_ptr)
{
	if ( !parse_token(input_ptr, "__typeof__") )
		return false;
	const char* input = *input_ptr;
	size_t i = 0;
	while ( input[i] && isspace((unsigned char) input[i]) )
		i++;
	if ( input[i] != '(' )
		return false;
	i++;
	size_t depth = 1;
	for ( ; input[i] && depth; i++ )
	{
		if ( input[i] == '(' )
			depth++;
		else if ( input[i] == ')' )
			depth--;
	}
	*input_ptr = input + i;
	parse_space(input_ptr);
	return !depth;
}

static bool parse_body(const char** input_ptr)
{
	size_t i = 0;
	const char* input = *input_ptr;
	if ( input[i] != '{' )
		return false;
	i++;
	size_t depth = 1;
	for ( ; input[i] && depth; i++ )
	{
		if ( input[i] == '{' )
			depth++;
		else if ( input[i] == '}' )
			depth--;
	}
	*input_ptr = input + i;
	return !depth;
}

static bool parse_attributes(const char** input_ptr)
{
	bool any = false;
	while ( parse_new_attribute(input_ptr) ||
	        parse_attribute(input_ptr) ||
	        parse_asm(input_ptr) )
		any = true;
	return any;
}

static char* parse_identifier(const char** input_ptr)
{
	size_t length = 0;
	while ( is_identifier((*input_ptr)[length]) )
		length++;
	if ( !length )
		return false;
	char* output = strndup(*input_ptr, length);
	if ( !output )
		err(1, "malloc");
	*input_ptr = *input_ptr + length;
	while ( isspace((unsigned char) **input_ptr) )
		(*input_ptr)++;
	return output;
}

static bool parse_preprocessor(const char** input_ptr)
{
	const char* from = *input_ptr;
	if ( !parse_char(input_ptr, '#') )
		return false;
	if ( parse_token(input_ptr, "define") )
	{
		char* identifier = parse_identifier(input_ptr);
		if ( !identifier )
			return false;
		found_name(identifier, TYPE_DEFINITION);
		free(identifier);
	}
	while ( **input_ptr && **input_ptr != '\n' )
		(*input_ptr)++;
	if ( polluted )
	{
		polluted = false;
		size_t length = *input_ptr - from;
		output_collapsed_space(from, length, stdout);
		printf("\n\n");
	}
	parse_char(input_ptr, '\n');
	return true;
}

static bool parse_static_assert(const char** input_ptr)
{
	if ( !parse_token(input_ptr, "_Static_assert") )
		return false;
	while ( **input_ptr && **input_ptr != ';' )
		(*input_ptr)++;
	return parse_char(input_ptr, ';');
}

static bool parse_enum_body(const char** input_ptr)
{
	while ( **input_ptr != '}' )
	{
		parse_attributes(input_ptr);
		char* identifier = parse_identifier(input_ptr);
		if ( !identifier )
			return false;
		found_name(identifier, TYPE_ENUMERATION_MEMBER);
		free(identifier);
		parse_attributes(input_ptr);
		if ( parse_char(input_ptr, '=') )
		{
			while ( **input_ptr && **input_ptr != ',' && **input_ptr != '}' )
				(*input_ptr)++;
		}
		if ( !parse_char(input_ptr, ',') )
			break;
	}
	return true;
}

static bool parse_decl(const char** input_ptr, bool argument, enum type type);

static bool parse_struct(const char** input_ptr)
{
	bool is_struct = false, is_enum = false, is_union = false;
	if ( !(is_struct = parse_token(input_ptr, "struct")) &&
	     !(is_enum = parse_token(input_ptr, "enum")) &&
	     !(is_union = parse_token(input_ptr, "union")) )
		return false;
	enum type type = TYPE_COUNT;
	if ( is_struct )
		type = TYPE_STRUCTURE;
	else if ( is_enum )
		type = TYPE_ENUMERATION;
	else if ( is_union )
		type = TYPE_UNION;
	parse_attributes(input_ptr);
	while ( isspace((unsigned char) **input_ptr) )
		(*input_ptr)++;
	char* name = parse_identifier(input_ptr);
	parse_attributes(input_ptr);
	if ( parse_char(input_ptr, '{') )
	{
		bool was_polluted = polluted;
		polluted = false;
		if ( is_enum )
		{
			if ( !parse_enum_body(input_ptr) )
				return free(name), false;
		}
		else
		{
			while ( **input_ptr != '}' )
			{
				enum type type = is_struct ? TYPE_STRUCTURE_MEMBER :
					                         TYPE_UNION_MEMBER;
				if ( !parse_decl(input_ptr, false, type) )
					return free(name), false;
			}
		}
		if ( !parse_char(input_ptr, '}') )
			return free(name), false;
		polluted = was_polluted;
	}
	if ( name )
	{
		found_name(name, type);
		free(name);
	}
	return true;
}

static bool parse_pointerness(const char** input_ptr)
{
	while ( **input_ptr )
	{
		if ( parse_token(input_ptr, "const") )
			;
		else if ( parse_token(input_ptr, "__const") )
			;
		else if ( parse_token(input_ptr, "restrict") )
			;
		else if ( parse_token(input_ptr, "__restrict") )
			;
		else if ( parse_token(input_ptr, "__restrict__") )
			;
		else if ( parse_token(input_ptr, "_Nonnull") )
			;
		else if ( parse_token(input_ptr, "_Nullable") )
			;
		else if ( parse_token(input_ptr, "volatile") )
			;
		else if ( parse_char(input_ptr, '*') )
			;
		// Weird Apple extension that is somehow used in c17 mode.
		else if ( parse_char(input_ptr, '^') )
			;
		else if ( isspace((unsigned char) **input_ptr) )
			(*input_ptr)++;
		else
			break;
	}
	return true;
}

static bool parse_arguments(const char** input_ptr)
{
	if ( **input_ptr != '(' )
		return false;
	(*input_ptr)++;
	while ( isspace((unsigned char) **input_ptr) )
		(*input_ptr)++;
	while ( **input_ptr != ')' )
	{
		if ( !**input_ptr )
			return false;
		if ( !strncmp(*input_ptr, "...", 3) )
		{
			(*input_ptr) += 3;
			parse_space(input_ptr);
			break;
		}
		if ( !parse_decl(input_ptr, true, TYPE_COUNT) )
			return false;
		if ( !parse_char(input_ptr, ',') )
			break;
	}
	if ( **input_ptr != ')' )
		return false;
	(*input_ptr)++;
	while ( isspace((unsigned char) **input_ptr) )
		(*input_ptr)++;
	return true;
}

static bool parse_decl(const char** input_ptr, bool argument, enum type type)
{
	if ( !argument && parse_char(input_ptr, ';') )
		return true;
	// TODO: Function argument type.
	const char* from = *input_ptr;
	if ( parse_token(input_ptr, "typedef") && type == TYPE_COUNT )
	     type = TYPE_TYPE;
	if ( type == TYPE_COUNT && !argument )
		type = TYPE_EXTERNAL;
	bool had_atomic = false;
	bool had_atomic_open = false;
	while ( parse_token(input_ptr, "__extension__") ||
	        parse_token(input_ptr, "static") ||
	        parse_token(input_ptr, "extern") ||
	        parse_token(input_ptr, "inline") ||
	        parse_token(input_ptr, "__inline") ||
	        parse_token(input_ptr, "__inline__") ||
	        parse_token(input_ptr, "__thread") ||
	        parse_token(input_ptr, "__thread__") ||
	        parse_token(input_ptr, "_Thread_local") ||
	        parse_token(input_ptr, "volatile") ||
	        parse_token(input_ptr, "register") ||
	        (parse_token(input_ptr, "_Atomic") &&
	         (had_atomic = true)) ||
	        (had_atomic && parse_char(input_ptr, '(') &&
	         (had_atomic_open = true)) ||
	        (had_atomic_open && parse_char(input_ptr, ')') &&
	         !(had_atomic = had_atomic_open = false)) ||
	        parse_token(input_ptr, "_Noreturn") ||
	        parse_token(input_ptr, "const") ||
	        parse_token(input_ptr, "__const") ||
	        parse_attributes(input_ptr) )
		;
	if ( next_is_token(*input_ptr, "struct") ||
	     next_is_token(*input_ptr, "enum") ||
	     next_is_token(*input_ptr, "union") )
	{
		if ( !parse_struct(input_ptr) )
			return false;
	}
	else if ( next_is_token(*input_ptr, "__typeof__") )
	{
		if ( !parse_typeof(input_ptr) )
			return false;
	}
	else
	{
		bool builtin_type = false;
		while ( parse_token(input_ptr, "unsigned") ||
		        parse_token(input_ptr, "signed") ||
		        parse_token(input_ptr, "char") ||
		        parse_token(input_ptr, "short") ||
		        parse_token(input_ptr, "int") ||
		        parse_token(input_ptr, "long") ||
		        parse_token(input_ptr, "float") ||
		        parse_token(input_ptr, "double") ||
		        parse_token(input_ptr, "void") ||
		        (parse_token(input_ptr, "_Atomic") && (had_atomic = true)) ||
		        (had_atomic && parse_char(input_ptr, '(') && (had_atomic_open = true)) ||
		        (had_atomic_open && parse_char(input_ptr, ')') && !(had_atomic = had_atomic_open = false)) ||
		        parse_token(input_ptr, "_Bool") ||
		        parse_token(input_ptr, "_Complex") )
			builtin_type = true;
		if ( !builtin_type )
		{
			char* name = parse_identifier(input_ptr);
			if ( !name )
				return false;
			free(name);
			if ( had_atomic_open )
				parse_char(input_ptr, ')');
		}
	}
	while ( true )
	{
		parse_pointerness(input_ptr);
		parse_attributes(input_ptr);
		char* name = NULL;
		if ( parse_char(input_ptr, '(') )
		{
			parse_pointerness(input_ptr);
			name = parse_identifier(input_ptr);
			if ( parse_char(input_ptr, '[') && !parse_char(input_ptr, ']') )
				return free(name), false;
			if ( **input_ptr == '(' )
			{
				if ( !parse_arguments(input_ptr) )
					return free(name), false;
			}
			if ( !parse_char(input_ptr, ')') )
				return free(name), false;
		}
		else
		{
			name = parse_identifier(input_ptr);
		}
		while ( parse_char(input_ptr, '[') )
		{
			while ( **input_ptr && **input_ptr != ']' )
				(*input_ptr)++;
			if ( !parse_char(input_ptr, ']') )
				return free(name), false;
		}
		parse_attributes(input_ptr);
		bool has_arguments = parse_arguments(input_ptr);
		if ( type == TYPE_EXTERNAL && has_arguments )
			type = TYPE_FUNCTION;
		parse_attributes(input_ptr);
		if ( name )
		{
			found_name(name, type);
			free(name);
		}
		if ( has_arguments && **input_ptr == '{' )
		{
			if ( !parse_body(input_ptr) )
				return false;
		}
		else
		{
			if ( parse_char(input_ptr, '=') || parse_char(input_ptr, ':') )
			{
				size_t depth = 0;
				while ( **input_ptr &&
					    (depth ||
					     (**input_ptr != ';' &&
					      **input_ptr != ',' &&
					      **input_ptr != '}')) )
				{
					if ( **input_ptr == '{' )
						depth++;
					else if ( **input_ptr == '}' )
						depth--;
					(*input_ptr)++;
				}
			}
			if ( !argument && parse_char(input_ptr, ',') )
				continue;
			if ( !argument )
			{
				if ( !parse_char(input_ptr, ';') )
					return false;
			}
		}
		break;
	}
	if ( !argument && polluted )
	{
		polluted = false;
		size_t length = *input_ptr - from;
		output_collapsed_space(from, length, stdout);
		printf("\n\n");
	}
	return true;
}

static bool parse_top(const char** input_ptr)
{
	while ( isspace((unsigned char) **input_ptr) )
		(*input_ptr)++;
	// Work around FreeBSD bug in devctl.h.
	if ( parse_token(input_ptr, "__BEGIN_DECLS") ||
	     parse_token(input_ptr, "__END_DECLS") )
		return true;
	while ( parse_token(input_ptr, "__extension__") )
		;
	if ( next_is_token(*input_ptr, "_Static_assert") )
		return parse_static_assert(input_ptr);
	if ( next_is_token(*input_ptr, "__asm") ||
	     next_is_token(*input_ptr, "__asm__") )
	{
		if ( !parse_asm(input_ptr) )
			return false;
		return parse_char(input_ptr, ';');
	}

	if ( !parse_decl(input_ptr, false, TYPE_COUNT) )
		return false;
	return true;
}

static void parse(const char* path, const char* input)
{
	while ( *input )
	{
		const char* current = input;
		if ( isspace((unsigned char) *input) )
			input++;
		else if ( parse_preprocessor(&input) )
			;
		else if ( parse_top(&input) )
			;
		else
			errx(1, "%s: syntax error: %.*s\n",
			     path, (int) strcspn(current, "\n"), current);
	}
}

static void remove_preprocessor_files(char* input)
{
	while ( *input )
	{
		if ( *input == '#' )
		{
			char* from = input;
			input++;
			while ( *input && isspace((unsigned char) *input) )
				input++;
			if ( !*input || !isdigit((unsigned char) *input) )
				continue;
			while ( *input && *input != '\n' )
				input++;
			memset(from, ' ', input - from);
		}
		else
			input++;
	}
}

static void load_api_recursive(const char* include_dir, const char* header,
                               char*** headers, size_t* headers_used,
                               size_t* headers_length,
                               const char* effective_options)
{
	for ( size_t i = 0; i < *headers_used; i++ )
	{
		if ( !strcmp((*headers)[i], header) )
			return;
	}
	char* copy = strdup(header);
	if ( !copy )
		err(1, "malloc");
	if ( !array_add(headers, headers_used, headers_length, copy) )
		err(1, "malloc");
	char* file;
	if ( format_string(&file, "%s.api", header) < 0 )
		err(1, "malloc");
	char* path = join_paths(include_dir, file);
	FILE* fp = fopen(path, "r");
	if ( !fp )
		err(1, "%s", path);
	char* line = NULL;
	size_t line_size = 0;
	ssize_t line_length;
	char* header_options = strdup("");
	if ( !header_options )
		err(1, "malloc");
	while ( 0 < (line_length = getline(&line, &line_size, fp)) )
	{
		if ( line[line_length-1] == '\n' )
			line[--line_length] = '\0';
		if ( strchr(line, '#') )
		{
			if ( line[0] == '[' )
			{
				free(header_options);
				header_options = strndup(line + 1, strcspn(line + 1, "]"));
				if ( !header_options )
					err(1, "malloc");
			}
			continue;
		}
		struct declaration* declaration = parse_declaration(line);
		if ( !declaration )
			errx(1, "invalid declaration: %s", line);
		struct declaration** new_declarations =
			realloc(declarations,
			        sizeof(struct declaration*) * (declarations_used + 1));
		if ( !new_declarations )
			err(1, "malloc");
		declarations = new_declarations;
		declarations[declarations_used++] = declaration;
		// Strip options when a XSI-only header is included from base POSIX.
		// This can happen for <sys/uio.h> via <sys/socket.h> optionally.
		if ( effective_options &&
		     strcmp(header_options, effective_options) != 0 &&
		     declaration->options &&
		     !strcmp(declaration->options, header_options) )
		{
			free(declaration->options);
			declaration->options = NULL;
		}
		if ( declaration->type_mask == REQUIRED_TYPE(TYPE_INCLUDE) )
		{
			char* decl_header = strdup(declaration->sig);
			if ( !decl_header )
				err(1, "malloc");
			for ( size_t i = 0; decl_header[i]; i++ )
			{
				if ( decl_header[i] == '/' )
					decl_header[i] = '_';
				else if ( decl_header[i] == '.' )
					decl_header[i] = '\0';
			}
			const char* options =
				declaration->options ? declaration->options : header_options;
			load_api_recursive(include_dir, decl_header, headers, headers_used,
					           headers_length, options);
			free(decl_header);
		}
	}
	free(header_options);
	free(line);
	if ( ferror(fp) )
		err(1, "getline: %s", path);
	fclose(fp);
	free(path);
	free(file);
}

static void load_api(const char* include_dir, const char* header)
{
	char** headers = NULL;
	size_t headers_used = 0;
	size_t headers_length = 0;
	load_api_recursive(include_dir, header, &headers, &headers_used,
	                   &headers_length, NULL);
	for ( size_t i = 0; i < headers_used; i++ )
		free(headers[i]);
	free(headers);
}

int main(int argc, char* argv[])
{
	const char* header = NULL;
	const char* error_path = NULL;
	const char* include_dir = NULL;
	const char* output_path = NULL;

	int opt;
	while ( (opt = getopt(argc, argv, "e:i:I:o:x")) != -1 )
	{
		switch ( opt )
		{
		case 'e': error_path = optarg; break;
		case 'i': header = optarg; break;
		case 'I': include_dir = optarg; break;
		case 'o': output_path = optarg; break;
		case 'x': xsi = true; break;
		default: return 1;
		}
	}

	if ( argc - optind < 1 )
		errx(1, "expected input path");

	mkdir_parent_of(error_path, 0777);
	mkdir_parent_of(output_path, 0777);

	if ( error_path && !freopen(error_path, "w", stdout) )
		err(1, "%s", error_path);

	load_api(include_dir, header);

	const char* outcome = NULL;

	for ( int i = optind; i < argc; i++ )
	{
		const char* path = argv[i];
		char* input = read_text_file(path);
		if ( !input )
			err(1, "%s", path);
		if ( !strcmp(input, "missing_header\n") )
		{
			fputs(input, stdout);
			outcome = "missing_header";
			break;
		}
		remove_preprocessor_files(input);
		parse(path, input);
		free(input);
	}

	if ( !ftello(stdout) )
	{
		if ( unlink(error_path) < 0 )
			err(1, "unlink: %s", error_path);
	}
	else if ( !outcome )
		outcome = "pollution";

	if ( !outcome )
		outcome = "good";

	FILE* out = fopen(output_path, "w");
	if ( !out )
		err(1, "%s", output_path);
	fprintf(out, "%s\n", outcome);
	if ( ferror(out) || fflush(out) == EOF )
		err(1, "write: %s", output_path);
	fclose(out);

	return 0;
}
