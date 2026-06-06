/*
 * Copyright (c) 2025, 2026 Jonas 'Sortie' Termansen.
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
 * genbasic.c
 * Generate the basic test suite, with templates for normal functions, and with
 * with code generation for the basic math and basic complex suites.
 */

// To generate template stubs for new functions in a new header:
//
//    cc misc/genbasic.c -o misc/genbasic
//    misc/genbasic include/foo.api
//
// The basic math suites are generated in two phases. The first phase collects
// training data from glibc using _Float128 precision math, and truncates the
// results to float/double/loing double and outputs the results as golden
// reference data. This strategy only works on glibc, or any system with
// a exceptionally precise math solution, making it possible to compute bit
// perfect long double expectations. glibc _Float128 math is *not* perfect, but
// the error is limited to the about ~13.0 unit-in-last-place (ULP) according to
// Gladman et al (2026), so the truncation to float/double/long double is bit
// perfect.
//
// To regenerate the basic math and basic complex suites:
//
// 1. Compile genbasic in training mode:
//
//    cc -DTRAIN misc/genbasic.c -o misc/genbasic
//
// 2. Generate the training tests:
//
//    misc/genbasic include/math.api include/complex.api
//
// 3. Run the basic test suite and put the expectations in out/golden:
//
//    make -C basic OUT_OS=golden test
//
// 4. Compile genbasic in golden mode:
//
//    cc -DGENERATE misc/genbasic.c -o misc/genbasic -lm
//
// 5. Generate the final tests:
//
//    misc/genbasic include/math.api include/complex.api
//
// If there are any special considerations for the functions, or the training
// libc is wrong in some cases, then the tests can be adjusted and fixed using
// the mathflags array below.

#include <sys/stat.h>

#include <ctype.h>
#include <errno.h>
#include <libgen.h>
#include <limits.h>
#include <math.h>
#include <regex.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "errors.h"

#if defined(TRAIN) || defined(GENERATE)

#define VAR_ANY -1
#define VAR_POS 0 // Positive input
#define VAR_NEG 1 // Negative input
#define VAR_NAN 2 // NaN input
#define VAR_POSINF 3 // Positive infinity input
#define VAR_NEGINF 4 // Negative infinity input
#define VAR_ZERO 5 // Zero input
#define VAR_MAX 6

// Skip this test variant for some reason.
#define MF_SKIP (1 << 0)
// Skip this test variant because its result is explicitly unspecified.
#define MF_UNSPEC MF_SKIP
// Skip this test variant because its result is implicitly unspecified and the
// issue should probably be reported to the standard, so it can explicitly say
// if the case is unspecified or standardize some behavior.
#define MF_OMITTED MF_SKIP
// Skip this test variant because its result is undefined.
#define MF_UNDEF MF_SKIP
// Skip this test variant because its result is implementation defined.
#define MF_IMPLDEF MF_SKIP
// Skip this test variant because we need to interpret the standard to judge it.
#define MF_INTERPRET MF_SKIP
// The invalid error must happen.
#define MF_FPINVALID (1 << 1)
// The overflow error must happen.
#define MF_FPOVERFLOW (1 << 2)
// The underflow error must happen.
#define MF_FPUNDERFLOW (1 << 3)
// The division by zero error must happen.
#define MF_FPDIVBYZERO (1 << 4)
// The error flag is allowed to happen, or not happen.
#define MF_MAYERR (1 << 5)
// An error is not allowed to happen (regardless of training data).
#define MF_NOERR (1 << 6)
// Don't skip the variant, but the first output (not errors) is unspecified.
#define MF_UNSPEC1 (1 << 7)
// Don't skip the variant, but the second output (not errors) is unspecified.
#define MF_UNSPEC2 (1 << 8)
// Any sign is allowed on the first output.
#define MF_ANYSIGN1 (1 << 9)
// Any sign is allowed on the second output.
#define MF_ANYSIGN2 (1 << 10)

struct mathflag
{
	const char* name;
	int variants[4];
	int flags;
};

static const struct mathflag mathflags[] =
{
	// POSIX gave up specifying the truth table of what happens on non-finite
	// inputs to cpow, since it explodes in size. pow already has 16 such
	// clauses so presumably cpow would need 256 clauses. Although this function
	// is interesting to study and the behavior should really be standardized
	// somewhere, we can't fail implementations here when there is no spec.
	{"cpow", {VAR_NEG, VAR_ANY, VAR_ANY, VAR_ANY}, MF_UNSPEC},
	{"cpow", {VAR_NAN, VAR_ANY, VAR_ANY, VAR_ANY}, MF_UNSPEC},
	{"cpow", {VAR_POSINF, VAR_ANY, VAR_ANY, VAR_ANY}, MF_UNSPEC},
	{"cpow", {VAR_NEGINF, VAR_ANY, VAR_ANY, VAR_ANY}, MF_UNSPEC},
	{"cpow", {VAR_ANY, VAR_NEG, VAR_ANY, VAR_ANY}, MF_UNSPEC},
	{"cpow", {VAR_ANY, VAR_NAN, VAR_ANY, VAR_ANY}, MF_UNSPEC},
	{"cpow", {VAR_ANY, VAR_POSINF, VAR_ANY, VAR_ANY}, MF_UNSPEC},
	{"cpow", {VAR_ANY, VAR_NEGINF, VAR_ANY, VAR_ANY}, MF_UNSPEC},
	{"cpow", {VAR_ANY, VAR_ANY, VAR_NEG, VAR_ANY}, MF_UNSPEC},
	{"cpow", {VAR_ANY, VAR_ANY, VAR_NAN, VAR_ANY}, MF_UNSPEC},
	{"cpow", {VAR_ANY, VAR_ANY, VAR_POSINF, VAR_ANY}, MF_UNSPEC},
	{"cpow", {VAR_ANY, VAR_ANY, VAR_NEGINF, VAR_ANY}, MF_UNSPEC},
	{"cpow", {VAR_ANY, VAR_ANY, VAR_ANY, VAR_NEG}, MF_UNSPEC},
	{"cpow", {VAR_ANY, VAR_ANY, VAR_ANY, VAR_NAN}, MF_UNSPEC},
	{"cpow", {VAR_ANY, VAR_ANY, VAR_ANY, VAR_POSINF}, MF_UNSPEC},
	{"cpow", {VAR_ANY, VAR_ANY, VAR_ANY, VAR_NEGINF}, MF_UNSPEC},
	{"cpow", {VAR_ZERO, VAR_ZERO, VAR_ZERO, VAR_ANY}, MF_UNSPEC},
	// Errors are optional in these cases.
	{"fma", {VAR_POSINF, VAR_ZERO, VAR_NAN}, MF_MAYERR | MF_FPINVALID},
	{"fma", {VAR_NEGINF, VAR_ZERO, VAR_NAN}, MF_MAYERR | MF_FPINVALID},
	{"fma", {VAR_ZERO, VAR_POSINF, VAR_NAN}, MF_MAYERR | MF_FPINVALID},
	{"fma", {VAR_ZERO, VAR_NEGINF, VAR_NAN}, MF_MAYERR | MF_FPINVALID},
	// Unspecified output with an invalid exception.
	{"lrint", {VAR_NAN}, MF_UNSPEC1 | MF_FPINVALID},
	{"lrint", {VAR_POSINF}, MF_UNSPEC1 | MF_FPINVALID},
	{"lrint", {VAR_NEGINF}, MF_UNSPEC1 | MF_FPINVALID},
	{"llrint", {VAR_NAN}, MF_UNSPEC1 | MF_FPINVALID},
	{"llrint", {VAR_POSINF}, MF_UNSPEC1 | MF_FPINVALID},
	{"llrint", {VAR_NEGINF}, MF_UNSPEC1 | MF_FPINVALID},
	{"lround", {VAR_NAN}, MF_UNSPEC1 | MF_FPINVALID},
	{"lround", {VAR_POSINF}, MF_UNSPEC1 | MF_FPINVALID},
	{"lround", {VAR_NEGINF}, MF_UNSPEC1 | MF_FPINVALID},
	{"llround", {VAR_NAN}, MF_UNSPEC1 | MF_FPINVALID},
	{"llround", {VAR_POSINF}, MF_UNSPEC1 | MF_FPINVALID},
	{"llround", {VAR_NEGINF}, MF_UNSPEC1 | MF_FPINVALID},
	// Unspecified second output without error.
	{"frexp", {VAR_NAN}, MF_UNSPEC2 | MF_NOERR},
	{"frexp", {VAR_POSINF}, MF_UNSPEC2 | MF_NOERR},
	{"frexp", {VAR_NEGINF}, MF_UNSPEC2 | MF_NOERR},
	// Any output sign is allowed in these cases.
	{"cacos", {VAR_POSINF, VAR_NAN}, MF_ANYSIGN2},
	{"cacos", {VAR_NEGINF, VAR_NAN}, MF_ANYSIGN2},
	{"cacosh", {VAR_ZERO, VAR_NAN}, MF_ANYSIGN2},
	{"casin", {VAR_POSINF, VAR_NAN}, MF_ANYSIGN2},
	{"casin", {VAR_NEGINF, VAR_NAN}, MF_ANYSIGN2},
	{"casin", {VAR_ZERO, VAR_NAN}, MF_ANYSIGN2},
	{"catan", {VAR_POSINF, VAR_NAN}, MF_ANYSIGN2},
	{"catan", {VAR_NEGINF, VAR_NAN}, MF_ANYSIGN2},
	{"csin", {VAR_NAN, VAR_POSINF}, MF_ANYSIGN2},
	{"csin", {VAR_NAN, VAR_NEGINF}, MF_ANYSIGN2},
	{"csin", {VAR_POSINF, VAR_POSINF}, MF_ANYSIGN2},
	{"csin", {VAR_NEGINF, VAR_POSINF}, MF_ANYSIGN2},
	{"csin", {VAR_POSINF, VAR_NEGINF}, MF_ANYSIGN2},
	{"csin", {VAR_NEGINF, VAR_NEGINF}, MF_ANYSIGN2},
	{"csinh", {VAR_NEGINF, VAR_NAN}, MF_ANYSIGN1},
	{"csinh", {VAR_NEGINF, VAR_POSINF}, MF_ANYSIGN1},
	{"csinh", {VAR_NEGINF, VAR_NEGINF}, MF_ANYSIGN1},
	// Correct for glibc bugs.
	{"lrint", {VAR_NAN, VAR_ANY}, MF_FPINVALID},
	{"lrint", {VAR_POSINF, VAR_ANY}, MF_FPINVALID},
	{"lrint", {VAR_NEGINF, VAR_ANY}, MF_FPINVALID},
	{"remquo", {VAR_NAN, VAR_ANY}, MF_UNSPEC2},
	{"remquo", {VAR_ANY, VAR_NAN}, MF_UNSPEC2},
	{"remquo", {VAR_POSINF, VAR_ANY}, MF_UNSPEC2},
	{"remquo", {VAR_NEGINF, VAR_ANY}, MF_UNSPEC2},
	{"remquo", {VAR_ANY, VAR_ZERO}, MF_UNSPEC2},
	{"remquo", {VAR_POSINF, VAR_POS}, MF_FPINVALID},
	{"remquo", {VAR_POSINF, VAR_NEG}, MF_FPINVALID},
	{"remquo", {VAR_POSINF, VAR_NAN}, MF_NOERR},
	{"remquo", {VAR_POSINF, VAR_POSINF}, MF_FPINVALID},
	{"remquo", {VAR_POSINF, VAR_NEGINF}, MF_FPINVALID},
	{"remquo", {VAR_POSINF, VAR_ZERO}, MF_FPINVALID},
	{"remquo", {VAR_NEGINF, VAR_POS}, MF_FPINVALID},
	{"remquo", {VAR_NEGINF, VAR_NEG}, MF_FPINVALID},
	{"remquo", {VAR_NEGINF, VAR_NAN}, MF_NOERR},
	{"remquo", {VAR_NEGINF, VAR_POSINF}, MF_FPINVALID},
	{"remquo", {VAR_NEGINF, VAR_NEGINF}, MF_FPINVALID},
	{"remquo", {VAR_NEGINF, VAR_ZERO}, MF_FPINVALID},
	{"remquo", {VAR_POS, VAR_ZERO}, MF_FPINVALID},
	{"remquo", {VAR_NEG, VAR_ZERO}, MF_FPINVALID},
	{"remquo", {VAR_ZERO, VAR_ZERO}, MF_FPINVALID},
	{"logb", {VAR_ZERO}, MF_MAYERR | MF_FPDIVBYZERO},
	// TODO: Austin Group Defect 714 changed the language for the positive infinity
	// input. However -inf remains unspecified.
	{"j0", {VAR_NEGINF}, MF_OMITTED},
	{"j1", {VAR_NEGINF}, MF_OMITTED},
	{"jn", {VAR_ANY, VAR_NEGINF}, MF_OMITTED},
	{"y0", {VAR_ZERO}, MF_MAYERR | MF_FPDIVBYZERO},
	{"y1", {VAR_ZERO}, MF_MAYERR | MF_FPDIVBYZERO},
	{"yn", {VAR_ANY, VAR_ZERO}, MF_MAYERR | MF_FPDIVBYZERO},
	{"y0", {VAR_NEG}, MF_MAYERR | MF_FPINVALID},
	{"y1", {VAR_NEG}, MF_MAYERR | MF_FPINVALID},
	{"yn", {VAR_ANY, VAR_NEG}, MF_MAYERR | MF_FPINVALID},
	{"y0", {VAR_NEGINF}, MF_OMITTED},
	{"y1", {VAR_NEGINF}, MF_OMITTED},
	{"yn", {VAR_ANY, VAR_NEGINF}, MF_OMITTED},
	// TODO: fdim is not specified on infinities.
	{"fdim", {VAR_POSINF, VAR_ANY}, MF_OMITTED},
	{"fdim", {VAR_NEGINF, VAR_ANY}, MF_OMITTED},
	{"fdim", {VAR_ANY, VAR_POSINF}, MF_OMITTED},
	{"fdim", {VAR_ANY, VAR_NEGINF}, MF_OMITTED},
	// TODO: Interpret whether -inf to the power of 13.37 is defined as a domain
	//       error case under "The value of x is negative and y is a finite
	//       non-integer."
	{"pow", {VAR_NEGINF, VAR_POS}, MF_INTERPRET},
	{"pow", {VAR_NEGINF, VAR_NEG}, MF_INTERPRET},
	{"pow", {VAR_NEGINF, VAR_POSINF}, MF_INTERPRET},
	{"pow", {VAR_NEGINF, VAR_NEGINF}, MF_INTERPRET},
	{"pow", {VAR_NEG, VAR_NEGINF}, MF_INTERPRET},
	{"pow", {VAR_NEG, VAR_POSINF}, MF_INTERPRET},
	{"pow", {VAR_ZERO, VAR_NEGINF}, MF_MAYERR | MF_FPDIVBYZERO},
};

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

static bool is_return_type(const char* sig, const char* type)
{
	if ( !strncmp(sig, "_Noreturn ", strlen("_Noreturn ")) )
		sig += strlen("_Noreturn ");
	if ( strncmp(sig, type, strlen(type)) != 0 )
		return false;
	sig += strlen(type);
	while ( *sig == ' ' )
		sig++;
	return is_identifier(*sig);
}

#if defined(GENERATE) || defined(TRAIN)
static bool is_type(const char* sig, const char* type)
{
	if ( strncmp(sig, type, strlen(type)) != 0 )
		return false;
	return true;
}

static const char* get_creal(const char* type)
{
	if ( !strcmp(type, "float complex") )
		return "crealf";
	else if ( !strcmp(type, "double complex") )
		return "creal";
	else if ( !strcmp(type, "long double complex") )
		return "creall";
	else
		return NULL;
}

static const char* get_cimag(const char* type)
{
	if ( !strcmp(type, "float complex") )
		return "cimagf";
	else if ( !strcmp(type, "double complex") )
		return "cimag";
	else if ( !strcmp(type, "long double complex") )
		return "cimagl";
	else
		return NULL;
}
static bool is_floating_type(const char* type)
{
	return strstr(type, "float") || strstr(type, "double");
}

static const char* decomplex(const char* type)
{
	if ( !strcmp(type, "float complex") )
		return "float";
	else if ( !strcmp(type, "double complex") )
		return "double";
	else if ( !strcmp(type, "long double complex") )
		return "long double";
	return type;
}
#endif

#ifdef GENERATE
static const char* get_suffix(const char* value, const char* type)
{
	if ( strchr(value, '(') || strchr(value, '?') )
		return "";
	if ( !strcmp(type, "long double") )
		return "L";
	else if ( !strcmp(type, "long") )
		return "L";
	else if ( !strcmp(type, "long long") )
		return "LL";
	else
		return "";
}

static void trim_zeroes(char* value)
{
	if ( value[0] == '-' )
		value++;
	if ( value[0] != '0' || value[1] != 'x' )
		return;
	size_t radix = strcspn(value, ".");
	if ( value[radix] != '.' || !isxdigit(value[radix+1]) )
		return;
	size_t p = radix + strcspn(value + radix, "p");
	if ( value[p] != 'p' )
		return;
	size_t o = p;
	while ( 2 <= o && value[o - 1] == '0' && value[o - 2] != '.' )
		o--;
	if ( o == p )
		return;
	size_t l = strlen(value + p);
	memmove(value + o, value + p, l + 1);
}
#endif

static void generate(const struct declaration* declaration,
                     const char* header)
{
	const char* name = declaration->name;
	bool is_math_function =
		(!strcmp(header, "complex.h") || !strcmp(header, "math.h")) &&
		 (is_return_type(declaration->sig, "float complex") ||
		  is_return_type(declaration->sig, "double complex") ||
		  is_return_type(declaration->sig, "long double complex") ||
		  is_return_type(declaration->sig, "float") ||
		  is_return_type(declaration->sig, "double") ||
		  is_return_type(declaration->sig, "long double") ||
		  is_return_type(declaration->sig, "int") ||
		  is_return_type(declaration->sig, "long") ||
		  is_return_type(declaration->sig, "long long")) &&
	    !strstr(declaration->sig, "char");
	bool autogenerate = is_math_function;
#if defined(TRAIN) || defined(GENERATE)
	if ( !autogenerate )
		return;
#else
	if ( autogenerate )
	{
		warnx("compile -DTRAIN or -DGENERATE to generate math functions: %s",
		      name);
		return;
	}
#endif

	char* header_name = strdup(header);
	if ( !header_name )
		err(1, "malloc");
	header_name[strlen(header_name) - 2] = '\0';
	for ( size_t i = 0; header_name[i]; i++ )
		if ( header_name[i] == '/' )
			header_name[i] = '_';
	char* subdir;
	char* path;
	if ( format_string(&subdir, "basic/%s", header_name) < 0 ||
	     format_string(&path, "%s/%s.c", subdir, declaration->name) < 0 )
		err(1, "malloc");
	if ( mkdir(subdir, 0777) < 0 && errno != EEXIST )
		err(1, "mkdir: %s", subdir);
	free(subdir);
	FILE* fp = fopen(path, autogenerate ? "w" : "wx");
	if ( !fp )
	{
		if ( errno == EEXIST )
		{
			warn("%s already exists", path);
			return;
		}
		err(1, "%s", path);
	}
	printf("%s\n", path);
	const char* invocation = strstr(declaration->sig, declaration->name);
	if ( declaration->options && strcmp(declaration->options, "CX") != 0 )
		fprintf(fp, "/*[%s]*/\n", declaration->options);
	fprintf(fp, "/* Test whether a basic %s invocation works. */\n",
	        declaration->name);
	if ( autogenerate )
		fprintf(fp, "/* This test is generated by misc/genbasic.c. */\n");
	fprintf(fp, "\n");
	// Use GNU extensions to get _Float128 math for training.
#ifdef TRAIN
	fprintf(fp, "#ifndef _GNU_SOURCE\n");
	fprintf(fp, "#define _GNU_SOURCE\n");
	fprintf(fp, "#endif\n");
	fprintf(fp, "\n");
#endif
	if ( !strcmp(header, "math.h") )
	{
		fprintf(fp, "#include <errno.h>\n");
		fprintf(fp, "#include <fenv.h>\n");
		if ( !strncmp(declaration->name, "next", 4) &&
		     strchr(declaration->name, 'l') )
			fprintf(fp, "#include <float.h>\n");
		if ( (!strcmp(name, "ilogb") ||
		     !strcmp(name, "ilogbf") ||
		     !strcmp(name, "ilogbl")) )
			fprintf(fp, "#include <limits.h>\n");
	}
	if ( !strcmp(header, "wctype.h") && strstr(declaration->name, "_l") )
		fprintf(fp, "#include <locale.h>\n");
	fprintf(fp, "#include <%s>\n", header);
	if ( !strcmp(header, "complex.h") )
	{
		fprintf(fp, "#include <errno.h>\n");
		fprintf(fp, "#include <fenv.h>\n");
		fprintf(fp, "#include <math.h>\n");
	}
	fprintf(fp, "\n");
	fprintf(fp, "#include \"../basic.h\"\n\n");
	// Compilers don't actually implement pragma FENV_ACCESS but the standard is
	// clear that it has to be used, and the compilers at least ignore the
	// pragma, so always include it.
	if ( !strcmp(header, "complex.h") || !strcmp(header, "math.h") )
		fprintf(fp, "#pragma STDC FENV_ACCESS ON\n\n");
	if ( !is_math_function )
	{
		fprintf(fp, "int main(void)\n");
		fprintf(fp, "{\n");
	}
	if ( is_math_function )
	{
#if defined(TRAIN) || defined(GENERATE)
		// Count the parameters to the math function.
		size_t parameters = 1;
		for ( size_t i = 0; declaration->sig[i]; i++ )
			if ( declaration->sig[i] == ',' )
				parameters++;
		// Don't count the pointer to the secondary output as a parameter.
		if ( !strcmp(name, "frexp") ||
		     !strcmp(name, "frexpf") ||
		     !strcmp(name, "frexpl") )
			parameters--;
		if ( !strcmp(name, "modf") ||
		     !strcmp(name, "modff") ||
		     !strcmp(name, "modfl") )
			parameters--;
		if ( !strcmp(name, "remquo") ||
		     !strcmp(name, "remquof") ||
		     !strcmp(name, "remquol") )
			parameters--;
#ifdef GENERATE
		// Read the golden expectation data from the out/golden directory.
		char* golden_path;
		if ( format_string(&golden_path, "out/golden/basic/%s/%s.out",
		                   header_name, name) < 0 )
			err(1, "malloc");
		FILE* golden = fopen(golden_path, "r");
		if ( !golden )
			err(1, "%s", golden_path);
#endif
		const char* outtype;
		const char* outfmt;
		const char* outdec;
		if ( is_return_type(declaration->sig, "float complex") )
			outtype = "float complex", outfmt = "%.6a", outdec = "%.8g";
		else if ( is_return_type(declaration->sig, "double complex") )
			outtype = "double complex", outfmt = "%.14a", outdec = "%.16g";
		else if ( is_return_type(declaration->sig, "long double complex") )
			outtype = "long double complex", outfmt = "%.17La", outdec = "%.20Lg";
		else if ( is_return_type(declaration->sig, "float") )
			outtype = "float", outfmt = "%.6a", outdec = "%.8g";
		else if ( is_return_type(declaration->sig, "double") )
			outtype = "double", outfmt = "%.14a", outdec = "%.16g";
		else if ( is_return_type(declaration->sig, "long double") )
			outtype = "long double", outfmt = "%.17La", outdec = "%.20Lg";
		else if ( is_return_type(declaration->sig, "int") )
			outtype = "int", outfmt = "%d", outdec = "%d";
		else if ( is_return_type(declaration->sig, "long long") )
			outtype = "long long", outfmt = "%lld", outdec = "%lld";
		else if ( is_return_type(declaration->sig, "long") )
			outtype = "long", outfmt = "%ld", outdec = "%ld";
		else
			errx(1, "unsupported type: %s", declaration->sig);
		const char* out2_type = NULL;
		const char* out2_fmt = NULL;
		const char* out2_name = NULL;
		if ( !strcmp(name, "frexp") || !strcmp(name, "frexpf") || !strcmp(name, "frexpl") )
			out2_type = "int", out2_fmt = "%d", out2_name = "exp";
		if ( !strcmp(name, "modf") || !strcmp(name, "modff") || !strcmp(name, "modfl") )
			out2_type = outtype, out2_fmt = outfmt, out2_name = "integral";
		if ( !strcmp(name, "remquo") || !strcmp(name, "remquof") || !strcmp(name, "remquol") )
			out2_type = "int", out2_fmt = "%d", out2_name = "quo";
		size_t m = 0;
		const char* intypes[3] = {NULL, NULL, NULL};
		const char* infmts[3] = {NULL, NULL, NULL};
		size_t param_offset = 0;
		for ( size_t i = 0; i < parameters; i++ )
		{
			char seek = i ? ',' : '(';
			while ( declaration->sig[param_offset] &&
			        declaration->sig[param_offset] != seek )
				param_offset++;
			if ( declaration->sig[param_offset] )
				param_offset++;
			while ( declaration->sig[param_offset] &&
			        declaration->sig[param_offset] == ' ' )
				param_offset++;
			const char* type = declaration->sig + param_offset;
			if ( is_type(type, "long double complex") )
			{
				intypes[i] = "long double complex";
				infmts[i] = "%.4Lf";
			}
			else if ( is_type(type, "double complex") )
			{
				intypes[i] = "double complex";
				infmts[i] = "%.4f";
			}
			else if ( is_type(type, "float complex") )
			{
				intypes[i] = "float complex";
				infmts[i] = "%.4f";
			}
			else if ( is_type(type, "long double") )
			{
				intypes[i] = "long double";
				infmts[i] = "%.4Lf";
			}
			else if ( is_type(type, "double") )
			{
				intypes[i] = "double";
				infmts[i] = "%.4f";
			}
			else if ( is_type(type, "float") )
			{
				intypes[i] = "float";
				infmts[i] = "%.4f";
			}
			else if ( is_type(type, "long long") )
			{
				intypes[i] = "long long";
				infmts[i] = "%lld";
			}
			else if ( is_type(type, "long") )
			{
				intypes[i] = "long";
				infmts[i] = "%ld";
			}
			else if ( is_type(type, "int") )
			{
				intypes[i] = "int";
				infmts[i] = "%d";
			}
			else
				errx(1, "unsupported type: %s", type);
		}
		fprintf(fp, "#define MF_UNSPEC1 (1 << 0)\n");
		fprintf(fp, "#define MF_UNSPEC2 (1 << 1)\n");
		fprintf(fp, "#define MF_MAYERR (1 << 2)\n");
		// This feature was not needed for math.h, so keep the generated code
		// shorter and only include it for complex.h functions.
		if ( !strcmp(header, "complex.h") )
		{
			fprintf(fp, "#define MF_ANYSIGN1 (1 << 2)\n");
			fprintf(fp, "#define MF_ANYSIGN2 (1 << 3)\n");
		}
		fprintf(fp, "\n");
		// The wrong error cases on edge cases are much more serious than benign
		// rounding errors, so if a test has both kinds of problem, make sure
		// only the first rounding error is output and then stop at the serious
		// error where the output is totally wrong. The wrong error handling is
		// much easier to fix than rounding errors, so we want to encourage
		// fixing those problems rather than hiding them.
		fprintf(fp, "// Soft fail on rounding errors and report only one.\n");
		fprintf(fp, "int imprecise;\n\n");
#ifdef GENERATE
		if ( !strcmp(header, "math.h") )
		{
			// We ignore FE_INEXACT for the purposes of this suite. It isn't as
			// clearly defined when it should/shouldn't happen, and we already
			// found a ton of bugs in the math library, and this suite is
			// already extremely complicated. We can amend this suite with
			// FE_INEXACT support in the future.
			fprintf(fp, "static const char* fperrname(int excepts)\n");
			fprintf(fp, "{\n");
			fprintf(fp, "	switch ( excepts )\n");
			fprintf(fp, "	{\n");
			fprintf(fp, "	case 0: return \"FE_NONE\";\n");
			fprintf(fp, "	case FE_INVALID: return \"FE_INVALID\";\n");
			fprintf(fp, "	case FE_DIVBYZERO: return \"FE_DIVBYZERO\";\n");
			fprintf(fp, "	case FE_OVERFLOW: return \"FE_OVERFLOW\";\n");
			fprintf(fp, "	case FE_UNDERFLOW: return \"FE_UNDERFLOW\";\n");
			fprintf(fp, "	default: return \"FE_MULTIPLE\";\n");
			fprintf(fp, "	}\n");
			fprintf(fp, "}\n");
			fprintf(fp, "\n");
		}
#endif
		fprintf(fp, "void test(int variant");
		for ( size_t i = 0; i < parameters; i++ )
			fprintf(fp, ", %s input%zu", intypes[i], i + 1);
#ifdef GENERATE
		if ( !strcmp(header, "math.h") )
		{
			fprintf(fp, ", int errnum");
			fprintf(fp, ", int fperr");
		}
		if ( is_floating_type(outtype) )
			fprintf(fp, ", %s lower", outtype);
		if ( out2_name && is_floating_type(out2_type) )
			fprintf(fp, ", %s lower_%s", out2_type, out2_name);
		fprintf(fp, ", %s expected", outtype);
		if ( out2_name )
			fprintf(fp, ", %s expected_%s", out2_type, out2_name);
		if ( is_floating_type(outtype) )
			fprintf(fp, ", %s upper", outtype);
		if ( out2_name && is_floating_type(out2_type) )
			fprintf(fp, ", %s upper_%s", out2_type, out2_name);
#endif
		fprintf(fp, ", int flags");
		fprintf(fp, ")\n");
		fprintf(fp, "{\n");
		if ( !strcmp(header, "math.h") )
		{
			fprintf(fp, "	errno = 0;\n");
			fprintf(fp, "	if ( feclearexcept(FE_ALL_EXCEPT) )\n");
			fprintf(fp, "		errx(1, \"feclearexcept\");\n");
		}
		bool float128_1 = false, float128_2 = false;
#ifdef TRAIN
		float128_1 = is_floating_type(outtype) && strncmp(name, "next", 4) != 0;
		float128_2 = out2_type && is_floating_type(out2_type);
#endif
		if ( out2_type )
		{
			if ( float128_2 )
				fprintf(fp, "	_Float128 %s128;\n", out2_name);
			fprintf(fp, "	%s %s;\n", out2_type, out2_name);
		}
		fprintf(fp, "	");
		fprintf(fp, "%s output = ", outtype);
		if ( float128_1 )
		{
			fprintf(fp, "(%s) ", outtype);
			int name_len = strlen(declaration->name);
			if ( !strcmp(decomplex(outtype), "float") ||
			     !strcmp(decomplex(outtype), "long double") )
				name_len--;
			fprintf(fp, "%.*sf128(", name_len, declaration->name);
		}
		else
			fprintf(fp, "%s(", declaration->name);
		for ( size_t p = 0; p < parameters; p++ )
			fprintf(fp, "%sinput%zu", p ? ", " : "", p + 1);
		if ( out2_name )
		{
			if ( float128_2 )
				fprintf(fp, ", &%s128", out2_name);
			else
				fprintf(fp, ", &%s", out2_name);
		}
		fprintf(fp, ");\n");
		if ( float128_2 )
			fprintf(fp, "\t%s = (%s) %s128;\n", out2_name, out2_type, out2_name);
#ifdef TRAIN
		// Collect the results of the test variant for later.
		fprintf(fp, "	(void) variant;\n");
		if ( !strcmp(header, "math.h") )
		{
			fprintf(fp, "	if ( errno )\n");
			fprintf(fp, "		printf(\"%%s\\n\", strerrno(errno));\n");
			fprintf(fp, "	else\n");
			fprintf(fp, "		printf(\"0\\n\");\n");
			fprintf(fp, "	if ( fetestexcept(FE_INVALID) )\n");
			fprintf(fp, "		printf(\"FE_INVALID\\n\");\n");
			fprintf(fp, "	else if ( fetestexcept(FE_DIVBYZERO) )\n");
			fprintf(fp, "		printf(\"FE_DIVBYZERO\\n\");\n");
			fprintf(fp, "	else if ( fetestexcept(FE_OVERFLOW) )\n");
			fprintf(fp, "		printf(\"FE_OVERFLOW\\n\");\n");
			fprintf(fp, "	else if ( fetestexcept(FE_UNDERFLOW) )\n");
			fprintf(fp, "		printf(\"FE_UNDERFLOW\\n\");\n");
			fprintf(fp, "	else\n");
			fprintf(fp, "		printf(\"0\\n\");\n");
		}
		(void) infmts;
		(void) outdec;
		(void) get_creal;
		(void) get_cimag;
		fprintf(fp, "\tif ( !(flags & MF_UNSPEC1) )\n");
		fprintf(fp, "\t{\n");
		if ( !strcmp(outtype, "float complex") )
		{
			fprintf(fp, "		printf(\"%s\\n\", crealf(output));\n", outfmt);
			fprintf(fp, "		printf(\"%s\\n\", cimagf(output));\n", outfmt);
		}
		else if ( !strcmp(outtype, "double complex") )
		{
			fprintf(fp, "		printf(\"%s\\n\", creal(output));\n", outfmt);
			fprintf(fp, "		printf(\"%s\\n\", cimag(output));\n", outfmt);
		}
		else if ( !strcmp(outtype, "long double complex") )
		{
			fprintf(fp, "		printf(\"%s\\n\", creall(output));\n", outfmt);
			fprintf(fp, "		printf(\"%s\\n\", cimagl(output));\n", outfmt);
		}
		else
			fprintf(fp, "		printf(\"%s\\n\", output);\n", outfmt);
		fprintf(fp, "\t}\n");
		if ( out2_name )
		{
			fprintf(fp, "\t\tif ( !(flags & MF_UNSPEC2) )\n");
			fprintf(fp, "\t\t{\n");
			if ( !strcmp(out2_type, "float complex") )
			{
				fprintf(fp, "		printf(\"%s\\n\", crealf(%s));\n", out2_fmt, out2_name);
				fprintf(fp, "		printf(\"%s\\n\", cimagf(%s));\n", out2_fmt, out2_name);
			}
			else if ( !strcmp(out2_type, "double complex") )
			{
				fprintf(fp, "		printf(\"%s\\n\", creal(%s));\n", out2_fmt, out2_name);
				fprintf(fp, "		printf(\"%s\\n\", cimag(%s));\n", out2_fmt, out2_name);
			}
			else if ( !strcmp(out2_type, "long double complex") )
			{
				fprintf(fp, "		printf(\"%s\\n\", creall(%s));\n", out2_fmt, out2_name);
				fprintf(fp, "		printf(\"%s\\n\", cimagl(%s));\n", out2_fmt, out2_name);
			}
			else
				fprintf(fp, "		printf(\"%s\\n\", %s);\n", out2_fmt, out2_name);
			fprintf(fp, "\t\t}\n");
		}
#elif defined(GENERATE)
		// Check if the test produced the right result.

		// First check the error handling.
		int outncount = strstr(outtype, "complex") || out2_type ? 2 : 1;
		for ( int j = 0; j < 4; j++ )
		{
			// complex.h does not use errno.
			if ( strcmp(header, "math.h") != 0 && j < 2 )
				continue;
			// TODO: A lot of "floating-point exception *may* be raised" for the
			// complex functions, which requires finer data. For now, ignore
			// such errors and focus on testing other aspects.
			else if ( !strcmp(header, "complex.h") )
				continue;
			const char* indent = "                     ";
			if ( j == 0 )
			{
				fprintf(fp, "	if ( errnum == 0 && errno )\n");
				fprintf(fp, "		err");
				indent = "                    ";
			}
			else if ( j == 1 )
			{
				fprintf(fp, "	if ( (math_errhandling & MATH_ERRNO) && errnum && errno != errnum )\n");
				fprintf(fp, "		errx");
			}
			else if ( j == 2 )
			{
				fprintf(fp, "	int excepts = fetestexcept(FE_INVALID | FE_DIVBYZERO | FE_OVERFLOW | FE_UNDERFLOW);\n");
				fprintf(fp, "	if ( fperr == 0 && excepts )\n");
				fprintf(fp, "		errx");
			}
			else if ( j == 3 )
			{
				fprintf(fp, "	if ( (math_errhandling & MATH_ERREXCEPT) && fperr != 0 &&\n");
				fprintf(fp, "	     excepts != fperr && !((flags & MF_MAYERR) && !excepts) )\n");
				fprintf(fp, "		errx");
			}
			fprintf(fp, "(1, \"(%%d.) %s(", declaration->name);
			for ( size_t p = 0; p < parameters; p++ )
			{
				if ( strstr(intypes[p], "complex") )
					fprintf(fp, "%s%s + i*%s", p ? ", " : "", infmts[p], infmts[p]);
				else
					fprintf(fp, "%s%s", p ? ", " : "", infmts[p]);
			}
			fprintf(fp, ")");
			if ( j == 0 )
				fprintf(fp, " failed");
			else if ( j == 1 )
				fprintf(fp, " did not %%s");
			else if ( j == 2 )
				fprintf(fp, " %%s");
			else if ( j == 3 )
				fprintf(fp, " did not %%s");
			fprintf(fp, "\"");
			fprintf(fp, ",\n%svariant", indent);
			for ( size_t p = 0; p < parameters; p++ )
			{
				if ( strstr(intypes[p], "complex") )
					fprintf(fp, ", %s(input%zu), %s(input%zu)", get_creal(intypes[p]), p + 1, get_cimag(intypes[p]), p + 1);
				else
					fprintf(fp, ", input%zu", p + 1);
			}
			if ( j == 1 )
				fprintf(fp, ", strerrno(errnum)");
			else if ( j == 2 )
				fprintf(fp, ", fperrname(excepts)");
			else if ( j == 3 )
				fprintf(fp, ", fperrname(fperr)");
			fprintf(fp, ");\n");
		}

		// Check the primary and secondary (if available) outputs.
		for ( int outn = 1; outn <= outncount; outn++ )
		{
			const char* outntype =
				strstr(outtype, "complex") ? outtype :
				outn == 1 ? outtype : out2_type;
			if ( strstr(outntype, "complex") )
			{
				if ( outn == 1 )
				{
					fprintf(fp, "\tif ( !(flags & MF_UNSPEC%d) )\n", outn);
					fprintf(fp, "\t{\n");
				}
			}
			else
			{

				fprintf(fp, "\tif ( !(flags & MF_UNSPEC%d) )\n", outn);
				fprintf(fp, "\t{\n");
			}
			const char* basic_type = decomplex(outntype);
			const char* to_test = outn == 1 ? "output" : out2_name;
			const char* test_fmt = outn == 1 ? outfmt : out2_fmt;
			const char* test_id = outn == 2 ? out2_name : "";
			const char* lower_pfx = "lower_";
			const char* expected_pfx = "expected_";
			const char* upper_pfx = "upper_";
			if ( strstr(outntype, "complex") )
			{
				to_test = "output";
				test_fmt = outfmt;
				test_id = outn == 1 ? "real" : "imag";
			}
			const char* lower_sfx = test_id;
			const char* expected_sfx = test_id;
			const char* upper_sfx = test_id;
			if ( !strcmp(expected_sfx, "") || !strcmp(expected_sfx, "output") )
			{
				lower_pfx = "lower", lower_sfx = "";
				expected_pfx = "expected", expected_sfx = "";
				upper_pfx = "upper", upper_sfx = "";
			}
			if ( strstr(outntype, "complex") && outn == 1 )
			{
				fprintf(fp, "\t\t%s real = %s(output);\n", basic_type, get_creal(outntype));
				fprintf(fp, "\t\treal = (flags & MF_ANYSIGN1) && real < 0.0 ? -real : real;\n");
				fprintf(fp, "\t\t%s lower_real = %s(lower);\n", basic_type, get_creal(outntype));
				fprintf(fp, "\t\t%s expected_real = %s(expected);\n", basic_type, get_creal(outntype));
				fprintf(fp, "\t\t%s upper_real = %s(upper);\n", basic_type, get_creal(outntype));
				to_test = "real";
			}
			else if ( strstr(outntype, "complex") && outn == 2 )
			{
				fprintf(fp, "\t\t%s imag = %s(output);\n", basic_type, get_cimag(outntype));
				fprintf(fp, "\t\timag = (flags & MF_ANYSIGN2) && imag < 0.0 ? -imag : imag;\n");
				fprintf(fp, "\t\t%s lower_imag = %s(lower);\n", basic_type, get_cimag(outntype));
				fprintf(fp, "\t\t%s expected_imag = %s(expected);\n", basic_type, get_cimag(outntype));
				fprintf(fp, "\t\t%s upper_imag = %s(upper);\n", basic_type, get_cimag(outntype));
				to_test = "imag";
			}
			bool did_ratio = false;
			if ( is_floating_type(basic_type) )
			{
				fprintf(fp, "\t\tif ( !(isnan(%s%s) ? isnan(%s) :\n",
				        expected_pfx, expected_sfx, to_test);
				fprintf(fp, "\t\t       isfinite(%s%s) && %s%s != 0.0 ?\n",
				        expected_pfx, expected_sfx, expected_pfx, expected_sfx);
				fprintf(fp, "\t\t       isfinite(%s) && (%s == %s%s || (%s%s < %s && %s < %s%s)) :\n",
				        to_test, to_test, expected_pfx, expected_sfx, lower_pfx, lower_sfx, to_test, to_test, upper_pfx, upper_sfx);
				fprintf(fp, "\t\t       %s == %s%s) )\n",
				        to_test, expected_pfx, expected_sfx);
				did_ratio = true;
			}
			else
				fprintf(fp, "\t\tif ( %s != %s%s )\n", to_test, expected_pfx, expected_sfx);
			if ( is_floating_type(basic_type) )
			{
				fprintf(fp, "\t\t{\n");
				fprintf(fp, "\t\t\tif ( imprecise && isfinite(%s) && isfinite(%s%s) )\n", to_test, expected_pfx, expected_sfx);
				fprintf(fp, "\t\t\t\treturn;\n");
				fprintf(fp, "\t\t\twarnx(\"(%%d.) %s(", declaration->name);
			}
			else
				fprintf(fp, "\t\t\terrx(1, \"(%%d.) %s(", declaration->name);
			for ( size_t p = 0; p < parameters; p++ )
			{
				if ( strstr(intypes[p], "complex") )
					fprintf(fp, "%s%s + i*%s", p ? ", " : "", infmts[p], infmts[p]);
				else
					fprintf(fp, "%s%s", p ? ", " : "", infmts[p]);
			}
			fprintf(fp, ")%s%s = %s, not %s", test_id[0] ? "." : "", test_id, test_fmt, test_fmt);
			if ( did_ratio )
				fprintf(fp, ", diff %s, ratio %s", test_fmt, outdec);
			fprintf(fp, "\",\n\t\t\t     ");
			fprintf(fp, "variant");
			for ( size_t p = 0; p < parameters; p++ )
			{
				if ( strstr(intypes[p], "complex") )
					fprintf(fp, ", %s(input%zu), %s(input%zu)", get_creal(intypes[p]), p + 1, get_cimag(intypes[p]), p + 1);
				else
					fprintf(fp, ", input%zu", p + 1);
			}
			fprintf(fp, ", %s, %s%s", to_test, expected_pfx, expected_sfx);
			if ( did_ratio )
			{
				fprintf(fp, ",\n\t\t\t     %s - %s%s", to_test, expected_pfx, expected_sfx);
				fprintf(fp, ", %s / %s%s", to_test, expected_pfx, expected_sfx);
			}
			fprintf(fp, ");\n");
			if ( is_floating_type(basic_type) )
			{
				fprintf(fp, "\t\t\tif ( !isfinite(%s) || !isfinite(%s%s) )\n", to_test, expected_pfx, expected_sfx);
				fprintf(fp, "\t\t\t\texit(1);\n");
				fprintf(fp, "\t\t\timprecise = 1;\n");
				fprintf(fp, "\t\t}\n");
			}
			if ( strstr(outntype, "complex") )
			{
				if ( outn == 2 )
					fprintf(fp, "\t}\n");
			}
			else
				fprintf(fp, "\t}\n");

		}
#endif
		fprintf(fp, "}\n\n");
		fprintf(fp, "int main(void)\n");
		fprintf(fp, "{\n");
		// Calculate how many test variants could exist given the inputs.
		size_t variant_count = 1;
		for ( size_t i = 0; i < parameters; i++ )
		{
			if ( !strcmp(intypes[i], "float") ||
			     !strcmp(intypes[i], "double") ||
			     !strcmp(intypes[i], "long double") )
				variant_count *= VAR_MAX;
			else if ( !strcmp(intypes[i], "float complex") ||
			          !strcmp(intypes[i], "double complex") ||
			          !strcmp(intypes[i], "long double complex") )
				variant_count *= VAR_MAX * VAR_MAX;
		}
		// Generate each test variant.
		for ( size_t variant = 0; variant < variant_count; variant++ )
		{
			size_t variants[4];
			size_t ps_count = 0;
			size_t variant_left = variant;
			const char* ps_types[4];
			// Assign parameters to inputs, as a complex input is actually two
			// parameters for testing purposes.
			for ( size_t i = 0; i < parameters; i++ )
			{
				if ( !strcmp(intypes[i], "float") ||
					 !strcmp(intypes[i], "double") ||
					 !strcmp(intypes[i], "long double") )
				{
					ps_types[ps_count] = decomplex(intypes[i]);
					variants[ps_count++] = variant_left % VAR_MAX;
					variant_left /= VAR_MAX;
				}
				else if ( !strcmp(intypes[i], "float complex") ||
					      !strcmp(intypes[i], "double complex") ||
					      !strcmp(intypes[i], "long double complex") )
				{
					ps_types[ps_count] = decomplex(intypes[i]);
					variants[ps_count++] = variant_left % VAR_MAX;
					variant_left /= VAR_MAX;
					ps_types[ps_count] = decomplex(intypes[i]);
					variants[ps_count++] = variant_left % VAR_MAX;
					variant_left /= VAR_MAX;
				}
				else
					variants[ps_count++] = 0;
			}
			// Look up the mathflags for this test variant.
			int flags = 0;
			size_t mathflags_count = sizeof(mathflags) / sizeof(mathflags[0]);
			for ( size_t i = 0; i < mathflags_count; i++ )
			{
				const struct mathflag* mf = &mathflags[i];
				size_t namelen = strlen(mf->name);
				if ( strncmp(name, mf->name, namelen) != 0 )
					continue;
				if ( strcmp(name + namelen, "f") != 0 &&
				     strcmp(name + namelen, "") != 0 &&
				     strcmp(name + namelen, "l") != 0 )
					continue;
				bool matching = true;
				for ( size_t p = 0; matching && p < ps_count; p++ )
				{
					if ( mf->variants[p] != VAR_ANY &&
				         mf->variants[p] != (int) variants[p] )
						matching = false;
				}
				if ( !matching )
					continue;
				flags |= mf->flags;
			}
			if ( flags & MF_SKIP )
				continue;
			m++;
			// Determine the exact parameters for this test variant. Generic
			// parameters are used by default, but some functions need special
			// values to result in the desired test coverage.
			// These arbitrary test values were chosen to have a reasonable
			// magnitude to not cause problems, and have a bit of decimal
			// precision to cause interesting bit patterns.
			const char* ps[4] = {"90.01", "13.37", "10.1", "4.2"};
			// Avoid overflow on float precision functions.
			if ( !strcmp(name, "expf") || !strcmp(name, "coshf") ||
			     !strcmp(name, "expm1f") || !strcmp(name, "sinhf") ||
			     !strcmp(name, "tgammaf") )
				ps[0] = "9.001";
			const char* neg = "-12.34";
			// For some of the math functions like catan, the defined strip is
			// limited on either the real or imaginary axis.
			if ( !strcmp(name, "acos") || !strcmp(name, "acosf") || !strcmp(name, "acosl") ||
			     !strcmp(name, "cacos") || !strcmp(name, "cacosf") || !strcmp(name, "cacosl") )
				ps[0] = "0.9001", ps[1] = "0.1337", neg = "-0.1234";
			if ( !strcmp(name, "asin") || !strcmp(name, "asinf") || !strcmp(name, "asinl") ||
			     !strcmp(name, "casin") || !strcmp(name, "casinf") || !strcmp(name, "casinl") )
				ps[0] = "0.9001", ps[1] = "0.1337", neg = "-0.1234";
			if ( !strcmp(name, "atanh") || !strcmp(name, "atanhf") || !strcmp(name, "atanhl") ||
			     !strcmp(name, "catanh") || !strcmp(name, "catanhf") || !strcmp(name, "catanhl") )
				ps[0] = "0.9001", ps[1] = "0.1337", neg = "-0.1234";
			if ( !strcmp(name, "erf") || !strcmp(name, "erff") || !strcmp(name, "erfl") )
				ps[0] = "1.01";
			if ( !strcmp(name, "erfc") || !strcmp(name, "erfcf") || !strcmp(name, "erfcl") )
				ps[0] = "1.01";
			if ( !strcmp(name, "jn") || !strcmp(name, "yn") )
				ps[0] = "5";
			if ( !strcmp(name, "ldexp") || !strcmp(name, "ldexpf") || !strcmp(name, "ldexpl") )
				ps[0] = "13";
			if ( !strcmp(name, "scalbn") || !strcmp(name, "scalbnf") || !strcmp(name, "scalbnl") )
				ps[0] = "13";
			if ( !strcmp(name, "scalbln") || !strcmp(name, "scalblnf") || !strcmp(name, "scalblnl") )
				ps[0] = "13";
			for ( size_t i = 0; i < ps_count; i++ )
			{
				if ( variants[i] == VAR_NEG )
					ps[i] = neg;
				else if ( !strcmp(ps_types[i], "float") && variants[i] == VAR_NAN )
					ps[i] = "nanf(\"\")";
				else if ( !strcmp(ps_types[i], "float") && variants[i] == VAR_POSINF )
					ps[i] = "strtof(\"inf\", NULL)";
				else if ( !strcmp(ps_types[i], "float") && variants[i] == VAR_NEGINF )
					ps[i] = "strtof(\"-inf\", NULL)";
				else if ( !strcmp(ps_types[i], "double") && variants[i] == VAR_NAN )
					ps[i] = "nan(\"\")";
				else if ( !strcmp(ps_types[i], "double") && variants[i] == VAR_POSINF )
					ps[i] = "strtod(\"inf\", NULL)";
				else if ( !strcmp(ps_types[i], "double") && variants[i] == VAR_NEGINF )
					ps[i] = "strtod(\"-inf\", NULL)";
				else if ( !strcmp(ps_types[i], "long double") && variants[i] == VAR_NAN )
					ps[i] = "nanl(\"\")";
				else if ( !strcmp(ps_types[i], "long double") && variants[i] == VAR_POSINF )
					ps[i] = "strtold(\"inf\", NULL)";
				else if ( !strcmp(ps_types[i], "long double") && variants[i] == VAR_NEGINF )
					ps[i] = "strtold(\"-inf\", NULL)";
				else if ( variants[i] == VAR_ZERO )
					ps[i] = "0.0";
			}
			// Avoid underflow.
			if ( (!strcmp(name, "erfc") || !strcmp(name, "erfcf") || !strcmp(name, "erfcl")) &&
			     variants[0] == VAR_NEG )
				ps[0] = "-0.1234";

			// Invoke the test variant with the inputs.
			fprintf(fp, "	test(%zu", m);
			for ( size_t p = 0; p < parameters; p++ )
			{
				if ( strstr(intypes[p], "complex") )
				{
					if ( !strcmp(intypes[p], "float complex") )
						fprintf(fp, ", CMPLXF(");
					else if ( !strcmp(intypes[p], "double complex") )
						fprintf(fp, ", CMPLX(");
					else if ( !strcmp(intypes[p], "long double complex") )
						fprintf(fp, ", CMPLXL(");
					fprintf(fp, "%s, %s)", ps[p*2+0], ps[p*2+1]);
				}
				else
					fprintf(fp, ", %s", ps[p]);
			}
#ifdef GENERATE
			// Include the test expectations from the golden file.

			char value[1024];

			if ( !strcmp(header, "math.h") )
			{
				fgets(value, sizeof(value), golden);
				value[strcspn(value, "\n")] = '\0';

				if ( flags & MF_NOERR )
					strcpy(value, "0");
				else if ( flags & MF_FPINVALID )
					strcpy(value, "EDOM");
				else if ( flags & MF_FPDIVBYZERO )
					strcpy(value, "ERANGE");
				else if ( flags & MF_FPOVERFLOW )
					strcpy(value, "ERANGE");
				else if ( flags & MF_FPUNDERFLOW )
					strcpy(value, "ERANGE");

				fprintf(fp, ", %s", value);
			}

			if ( !strcmp(header, "math.h") )
			{
				fgets(value, sizeof(value), golden);
				value[strcspn(value, "\n")] = '\0';

				if ( flags & MF_NOERR )
					strcpy(value, "0");
				else if ( flags & MF_FPINVALID )
					strcpy(value, "FE_INVALID");
				else if ( flags & MF_FPDIVBYZERO )
					strcpy(value, "FE_DIVBYZERO");
				else if ( flags & MF_FPOVERFLOW )
					strcpy(value, "FE_OVERFLOW");
				else if ( flags & MF_FPUNDERFLOW )
					strcpy(value, "FE_UNDERFLOW");

				fprintf(fp, ", %s", value);
			}

			char value1[1024] = "";
			char value2[1024] = "";

			if ( !(flags & MF_UNSPEC1) )
			{
				fgets(value1, sizeof(value1), golden);
				value1[strcspn(value1, "\n")] = '\0';
			}
			if ( 2 <= outncount )
			{
				if ( !(flags & (strstr(outtype, "complex") ? MF_UNSPEC1 : MF_UNSPEC2)) )
				{
					fgets(value2, sizeof(value2), golden);
					value2[strcspn(value2, "\n")] = '\0';
				}
			}

			// Calculate the lower bound (exclusive) for the allowed output,
			// calculate the expected value for the allowed output, and
			// calculate the upper bound (exclusive) for the allowed output.
			// The test passes if: lower < output < upper || output == expected.
			// This logic allows the floating point precision to be higher,
			// which will be between the lower and upper bound, or lower, which
			// will be the exact comparison due to truncation.
			for ( int bound = -1; bound <= 1; bound ++ )
			{
				for ( int outn = 1; outn <= outncount; outn++ )
				{
					const char* outntype =
						strstr(outtype, "complex") ? outtype :
						outn == 1 ? outtype : out2_type;
					const char* basic_type = decomplex(outntype);
					if ( !is_floating_type(basic_type) && bound != 0 )
						continue;
					const char* cmplx = NULL;
					if ( !strcmp(outntype, "float complex") )
						cmplx = "CMPLXF";
					else if ( !strcmp(outntype, "double complex") )
						cmplx = "CMPLX";
					else if ( !strcmp(outntype, "long double complex") )
						cmplx = "CMPLXL";
					if ( strstr(outntype, "complex") )
					{
						if ( outn == 1 )
							fprintf(fp, ", %s(", cmplx);
						if ( outn == 1 && (flags & MF_UNSPEC1) )
						{
							fprintf(fp, "0.0, 0.0)");
							continue;
						}
					}
					else
					{
						if ( outn == 1 && (flags & MF_UNSPEC1) )
						{
							fprintf(fp, ", 0");
							continue;
						}
						if ( outn == 2 && (flags & MF_UNSPEC2) )
						{
							fprintf(fp, ", 0");
							continue;
						}
					}
					strcpy(value, outn == 1 ? value1 : value2);
					// Drop the sign if the test variant allows any sign.
					if ( ((outn == 1 && (flags & MF_ANYSIGN1)) ||
					      (outn == 2 && (flags & MF_ANYSIGN2))) &&
					     value[0] == '-' )
						memmove(value, value + 1, strlen(value + 1) + 1);
					// Some functions return symbolic constants instead of
					// portable constant values, so replace those values with
					// the symbolic constant name.
					if ( outn == 1 )
					{
						if ( variants[0] == VAR_NAN &&
							 (!strcmp(name, "ilogb") || !strcmp(name, "ilogbf") || !strcmp(name, "ilogbl")) )
							 strcpy(value, "FP_ILOGBNAN");
						if ( (variants[0] == VAR_POSINF || variants[0] == VAR_NEGINF) &&
							 (!strcmp(name, "ilogb") || !strcmp(name, "ilogbf") || !strcmp(name, "ilogbl")) )
							 strcpy(value, "INT_MAX");
						if ( variants[0] == VAR_ZERO &&
							 (!strcmp(name, "ilogb") || !strcmp(name, "ilogbf") || !strcmp(name, "ilogbl")) )
							 strcpy(value, "FP_ILOGB0");
						if ( (!strcmp(name, "remquo") || !strcmp(name, "remquof") || !strcmp(name, "remquol") ) &&
							 (variants[0] == VAR_POSINF || variants[0] == VAR_NEGINF) )
							strcpy(value, "nan");
						if ( (!strcmp(name, "sin") || !strcmp(name, "sinf") || !strcmp(name, "sinl") ||
							  !strcmp(name, "cos") || !strcmp(name, "cosf") || !strcmp(name, "cosl") ||
							  !strcmp(name, "tan") || !strcmp(name, "tanf") || !strcmp(name, "tanl")) &&
							 (variants[0] == VAR_POSINF || variants[0] == VAR_NEGINF) )
							strcpy(value, "nan");
					}
					if ( !(strstr(outntype, "complex") && outn == 1) )
						fprintf(fp, ", ");
					// Calculate the lower and upper bounds for the expected
					// result. The good news is that glibc _Float128 math is
					// accurate to ~13.0 ULP according to Gladman et al (2026)
					// <https://members.loria.fr/PZimmermann/papers/accuracy.pdf>
					// which means that our _Float128 training data produces bit
					// perfect results for float/double/long double functions
					// after truncation. We can then use nextafter() to simply
					// set the lower and upper bounds to the adjacent floating
					// point number, and be extremely strict to require the
					// perfect mathematical result.
					if ( is_floating_type(basic_type) )
					{
						if ( !strchr(value, '.') )
						{
							// No adjustment needed for non-finite value.
						}
						else if ( bound != 0 && !strcmp(basic_type, "float") )
						{
							float f = strtof(value, NULL);
							float inf = strtof(bound < 0 ? "-inf" : "inf", NULL);
							float r = nextafterf(f, inf);
							snprintf(value, sizeof(value), "%.6a", r);
						}
						else if ( bound != 0 && !strcmp(basic_type, "double") )
						{
							double f = strtod(value, NULL);
							double inf = strtod(bound < 0 ? "-inf" : "inf", NULL);
							double r = bound == 0 ? f : nextafter(f, inf);
							// Except for jn/yn which the paper says has an
							// worst ULP error of 3.57e33 on glibc! UGH. So we
							// will instead allow super imprecise numbers. This
							// range allows every implementation except AIX. I'm
							// unclear how to determine what the proper numbers
							// are though, so this range may be wrong.
							if ( !strcmp(name, "j0") || !strcmp(name, "j1") || !strcmp(name, "jn") ||
							     !strcmp(name, "y0") || !strcmp(name, "y1") || !strcmp(name, "yn") )
							{
								bool is_lower = bound == -1;
								bool is_positive = 0.0 < f ? true : false;
								if ( f != 0.0 && is_lower == is_positive )
									r = f * 0.99999999999;
								else if ( f != 0.0 && is_lower != is_positive )
									r = f * 1.00000000001;
							}
							snprintf(value, sizeof(value), "%.14a", r);
						}
						// Allow double == long double.
						else if ( !strncmp(name, "next", 4) && !strcmp(basic_type, "long double") )
						{
							// The double representation case has to be
							// calculated separately here to be precise. We have
							// to essentially include a different bound for
							// each representation and use a conditional to use
							// the right bound per the preprocessor macros.
							double d = strtod(ps[0], NULL);
							if ( variants[0] == VAR_POSINF )
								d = strtod("inf", NULL);
							else if ( variants[0] == VAR_NEGINF )
								d = strtod("-inf", NULL);
							else if ( variants[0] == VAR_NAN )
								d = nan("");
							double ddir = strtod(ps[1], NULL);
							if ( variants[1] == VAR_POSINF )
								ddir = strtod("inf", NULL);
							else if ( variants[1] == VAR_NEGINF )
								ddir = strtod("-inf", NULL);
							else if ( variants[1] == VAR_NAN )
								ddir = nan("");
							d = nextafter(d, ddir);
							double dinf = strtod(bound < 0 ? "-inf" : "inf", NULL);
							double dr = bound == 0 ? d : nextafter(d, dinf);
							long double ld = strtold(value, NULL);
							long double ldinf = strtold(bound < 0 ? "-inf" : "inf", NULL);
							long double ldr = bound == 0 ? ld : nextafterl(ld, ldinf);
							char drstr[1024], ldrstr[1024];
							if ( isinf(dr) && dr < 0 )
								strcpy(drstr, "strtod(\"-inf\", NULL)");
							else if ( isinf(dr) && dr > 0 )
								strcpy(drstr, "strtod(\"inf\", NULL)");
							else if ( isnan(dr) )
								strcpy(drstr, "nan(\"\")");
							else
								snprintf(drstr, sizeof(drstr), "%.14a", dr);
							if ( isinf(ldr) && ldr < 0 )
								strcpy(ldrstr, "strtold(\"-inf\", NULL)");
							else if ( isinf(ldr) && ldr > 0 )
								strcpy(ldrstr, "strtold(\"inf\", NULL)");
							else
								snprintf(ldrstr, sizeof(ldrstr), "%.17La", ldr);
							snprintf(value, sizeof(value),
							         "DBL_MANT_DIG == LDBL_MANT_DIG ? %s : %s%s",
							         drstr, ldrstr, get_suffix(ldrstr, "long double"));
						}
						else if ( bound != 0 && !strcmp(basic_type, "long double") )
						{
							long double f = strtold(value, NULL);
							long double inf = strtold(bound < 0 ? "-inf" : "inf", NULL);
							long double r = nextafterl(f, inf);
							snprintf(value, sizeof(value), "%.17La", r);
						}
						if ( !strcmp(basic_type, "float") && !strcmp(value, "nan") )
							fprintf(fp, "nanf(\"\")");
						else if ( !strcmp(basic_type, "double") && !strcmp(value, "nan") )
							fprintf(fp, "nan(\"\")");
						else if ( !strcmp(basic_type, "long double") && !strcmp(value, "nan") )
							fprintf(fp, "nanl(\"\")");
						else if ( !strcmp(basic_type, "float") && !strcmp(value, "-nan") )
							fprintf(fp, "nanf(\"\")");
						else if ( !strcmp(basic_type, "double") && !strcmp(value, "-nan") )
							fprintf(fp, "nan(\"\")");
						else if ( !strcmp(basic_type, "long double") && !strcmp(value, "-nan") )
							fprintf(fp, "nanl(\"\")");
						else if ( !strcmp(basic_type, "float") && !strcmp(value, "inf") )
							fprintf(fp, "strtof(\"inf\", NULL)");
						else if ( !strcmp(basic_type, "double") && !strcmp(value, "inf") )
							fprintf(fp, "strtod(\"inf\", NULL)");
						else if ( !strcmp(basic_type, "long double") && !strcmp(value, "inf") )
							fprintf(fp, "strtold(\"inf\", NULL)");
						else if ( !strcmp(basic_type, "float") && !strcmp(value, "-inf") )
							fprintf(fp, "strtof(\"-inf\", NULL)");
						else if ( !strcmp(basic_type, "double") && !strcmp(value, "-inf") )
							fprintf(fp, "strtod(\"-inf\", NULL)");
						else if ( !strcmp(basic_type, "long double") && !strcmp(value, "-inf") )
							fprintf(fp, "strtold(\"-inf\", NULL)");
						else
						{
							trim_zeroes(value);
							if ( !strcmp(value, "inf") )
								errx(1, "unexpected value %s of type %s", value, basic_type);
							fprintf(fp, "%s%s", value, get_suffix(value, basic_type));
						}
					}
					else
					{
						if ( !strcmp(value, "inf") )
							errx(1, "value %s of type %s", value, basic_type);
						fprintf(fp, "%s%s", value, get_suffix(value, basic_type));
					}
					if ( strstr(outntype, "complex") && outn == 2 )
						fprintf(fp, ")");
				}
			}
#endif
			fprintf(fp, ", 0");
			if ( flags & MF_UNSPEC1 )
				fprintf(fp, " | MF_UNSPEC1");
			if ( flags & MF_UNSPEC2 )
				fprintf(fp, " | MF_UNSPEC2");
			if ( flags & MF_MAYERR )
				fprintf(fp, " | MF_MAYERR");
			if ( flags & MF_ANYSIGN1 )
				fprintf(fp, " | MF_ANYSIGN1");
			if ( flags & MF_ANYSIGN2 )
				fprintf(fp, " | MF_ANYSIGN2");
			fprintf(fp, ");\n");
		}
		// Fail in the end if a soft error occurred.
		fprintf(fp, "	return imprecise;\n");
		fprintf(fp, "}\n");
#endif
	}
	// Oh and this program also generates stubs for test cases for the other
	// headers. It just got totally taken over by the floating point stuff which
	// - uh - grew way beyond what I originally imagined.
	else if ( is_return_type(declaration->sig, "void") )
	{
		fprintf(fp, "	%s;\n", invocation);
	}
	else if ( !strcmp(header, "threads.h") &&
	          is_return_type(declaration->sig, "int") )
	{
		fprintf(fp, "	int ret = %s;\n", invocation);
		fprintf(fp, "	if ( ret != thrd_success )\n");
		fprintf(fp, "		errx(1, \"%s: %%s\",\n", declaration->name);
		fprintf(fp, "		     ret == thrd_busy ? \"thrd_busy\" :\n");
		fprintf(fp, "		     ret == thrd_nomem ? \"thrd_nomem\" :\n");
		fprintf(fp, "		     ret == thrd_timedout ? \"thrd_timedout\" :\n");
		fprintf(fp, "		     ret == thrd_error ? \"thrd_error\" :\n");
		fprintf(fp, "		     \"thrd_unknown\");\n");

	}
	else if ( !strcmp(header, "wctype.h") )
	{
		const char* param = "";
		if ( !strcmp(header, "wctype.h") && strstr(declaration->name, "_l") )
		{
			fprintf(fp, "	locale_t locale = duplocale(LC_GLOBAL_LOCALE);\n");
			fprintf(fp, "	if ( locale == (locale_t) 0 )\n");
			fprintf(fp, "		err(1, \"duplocale\");\n");
			param = ", locale";
		}
		fprintf(fp, "	wchar_t wc1 = L'';\n");
		fprintf(fp, "	wchar_t wc2 = L'';\n");
		fprintf(fp, "	if ( !%s(wc1%s) )\n", declaration->name, param);
		fprintf(fp, "		errx(1, \"%s(%%lc) was not true\", wc1);\n", declaration->name);
		fprintf(fp, "	if ( %s(wc2%s) )\n", declaration->name, param);
		fprintf(fp, "		errx(1, \"%s(%%lc) was not false\", wc2);\n", declaration->name);

	}
	else if ( (!strcmp(header, "pthread.h") || !strcmp(header, "spawn.h")) &&
	          is_return_type(declaration->sig, "int") )
	{
		fprintf(fp, "	if ( (errno = %s) )\n", invocation);
		fprintf(fp, "		err(1, \"%s\");\n", declaration->name);

	}
	else if ( is_return_type(declaration->sig, "int") ||
	          is_return_type(declaration->sig, "ssize_t") )
	{
		fprintf(fp, "	if ( %s < 0 )\n", invocation);
		fprintf(fp, "		err(1, \"%s\");\n", declaration->name);
	}
	else
	{
		fprintf(fp, "	%s;\n", declaration->sig);
		fprintf(fp, "	if ( TODO )\n");
		fprintf(fp, "		err(1, \"%s\");\n", declaration->name);
	}
	if ( !is_math_function )
	{
		fprintf(fp, "	return 0;\n");
		fprintf(fp, "}\n");
	}
	free(path);
}

int main(int argc, char* argv[])
{
	int opt;
	while ( (opt = getopt(argc, argv, "")) != -1 )
	{
		switch ( opt )
		{
		default: return 1;
		}
	}

	if ( argc - optind < 1 )
		errx(1, "expected input path");

	for ( int i = optind; i < argc; i++ )
	{
		const char* api_path = argv[i];
		FILE* api = fopen(api_path, "r");
		if ( !api )
			err(1, "%s", api_path);

		char* line = NULL;
		size_t line_size = 0;
		ssize_t line_length;
		char* header = NULL;
		while ( 0 < (line_length = getline(&line, &line_size, api)) )
		{
			if ( line[line_length-1] == '\n' )
				line[--line_length] = '\0';
			if ( strchr(line, '#') )
			{
				size_t offset = strcspn(line, "<") + 1;
				if ( !(header = strndup(line + offset,
					                    strlen(line + offset) - 1)) )
					err(1, "malloc");
				continue;
			}
			if ( !header )
				continue;
			struct declaration* declaration = parse_declaration(line);
			if ( !declaration )
				errx(1, "invalid declaration: %s", line);
			if ( declaration->type_mask &
				 (REQUIRED_TYPE(TYPE_FUNCTION) | OPTIONAL_TYPE(TYPE_FUNCTION)) )
				generate(declaration, header);
		}
		free(header);
		free(line);
		if ( ferror(api) )
			err(1, "getline: %s", api_path);
		fclose(api);
	}

	return 0;
}
