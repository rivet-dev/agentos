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
 * posix-parse.c
 * Parse POSIX html header definitions into machine readable data.
 */

// TODO: stdbool issues on C23
// TODO: posix 2024 blog post timespec_get and CMPLX{,F,L} is new
// TODO: CLOCKS_PER_SEC is not XSI

#include <sys/stat.h>

#include <ctype.h>
#include <err.h>
#include <errno.h>
#include <libgen.h>
#include <regex.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef COLOR
#define DEBUG_COLOR "\e[91m"
#define WARNING_COLOR "\e[92m"
#define OUTPUT_COLOR "\e[93m"
#define END_COLOR "\e[m"
#else
#define DEBUG_COLOR "//"
#define WARNING_COLOR "// "
#define OUTPUT_COLOR ""
#define END_COLOR ""
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

static size_t skipped = 0;
static size_t total = 0;

static char* header = NULL;
static char* header_options = NULL;
static struct declaration** declarations = NULL;
static size_t declarations_used = 0;
static size_t parent_id = 0;
static int following_type = REQUIRED_TYPE(TYPE_DEFINITION);
static bool following_optional = false;
static bool following_actually_definitions = false;
static char* following_options = NULL;
static size_t in_shall_define_from = 0;

static bool is_identifier(char c)
{
	return ('a' <= c && c <= 'z') || ('A' <= c && c <= 'Z') ||
	       ('0' <= c && c <= '9') || c == '_';
}

static bool next_is_token(const char* input, const char* token)
{
	return !strncmp(input, token, strlen(token)) &&
	       (!input[strlen(token)] ||
	        isspace((unsigned char) input[strlen(token)]));
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

static void output_declaration(struct declaration* declaration, FILE* fp)
{
	(void) parse_declaration;
	if ( declaration->options )
		fprintf(fp, "[%s] ", declaration->options);
	if ( declaration->optional )
		fprintf(fp, "optional ");
	if ( declaration->incomplete )
		fprintf(fp, "incomplete ");
	if ( declaration->parent )
		fprintf(fp, "parent %s ", declaration->parent);
	const char* s = "";
	for ( enum type type = TYPE_FIRST; type < TYPE_COUNT; type++ )
	{
		if ( declaration->type_mask & OPTIONAL_TYPE(type) )
			fprintf(fp, "%smaybe_%s", s, type_names[type]),
			s = " ";
		else if ( declaration->type_mask & REQUIRED_TYPE(type) )
			fprintf(fp, "%s%s", s, type_names[type]),
			s = " ";
	}
	if ( declaration->type_mask == REQUIRED_TYPE(TYPE_STRUCTURE) ||
	     declaration->type_mask == REQUIRED_TYPE(TYPE_UNION) )
		fprintf(fp, " %s;\n", declaration->name);
	else if ( declaration->type_mask == REQUIRED_TYPE(TYPE_NAMESPACE) )
		fprintf(fp, " %s;\n", declaration->sig);
	else
	{
	     if ( strcmp(declaration->name, declaration->sig) != 0 )
			fprintf(fp, " %s:", declaration->name);
		fprintf(fp, " %s;\n", declaration->sig);
	}
}

static struct declaration* add_declaration_to_parent(const char* sig,
                                                     int type_mask,
                                                     const char* parent)
{
	if ( following_actually_definitions )
	{
		if ( type_mask & REQUIRED_TYPE(TYPE_SYMBOLIC_CONSTANT) )
			type_mask |= REQUIRED_TYPE(TYPE_DEFINITION);
		type_mask &= ~REQUIRED_TYPE(TYPE_SYMBOLIC_CONSTANT);
	}
	struct declaration** new_declarations =
		realloc(declarations,
		        sizeof(struct declaration) * (declarations_used + 1));
	if ( !new_declarations )
		abort();
	declarations = new_declarations;
	struct declaration* declaration = calloc(1, sizeof(struct declaration));
	if ( !declaration )
		abort();
	declaration->optional = following_optional;
	declaration->type_mask = type_mask;
	if ( !strncmp(sig, "extern ", strlen("extern ")) )
		sig += strlen("extern ");
	if ( !(declaration->sig = strdup(sig)) )
		abort();
	if ( type_mask == REQUIRED_TYPE(TYPE_STRUCTURE) &&
	     strncmp(declaration->sig, "struct ", strlen("struct ")) != 0 )
	{
		free(declaration->sig);
		if ( asprintf(&declaration->sig, "struct %s", sig) < 0 )
			abort();
	}
	else if ( type_mask == REQUIRED_TYPE(TYPE_UNION) &&
	          strncmp(declaration->sig, "union ", strlen("union ")) != 0 )
	{
		free(declaration->sig);
		if ( asprintf(&declaration->sig, "union %s", sig) < 0 )
			abort();
	}
	else if ( type_mask == REQUIRED_TYPE(TYPE_ENUMERATION) &&
	          strncmp(declaration->sig, "enum ", strlen("enum ")) != 0 )
	{
		free(declaration->sig);
		if ( asprintf(&declaration->sig, "enum %s", sig) < 0 )
			abort();
	}
	size_t sig_len = strlen(declaration->sig);
	while ( sig_len && isspace((unsigned char) declaration->sig[sig_len-1]) )
		declaration->sig[--sig_len] = '\0';
	for ( size_t i = 0; i < sig_len; i++ )
		if ( declaration->sig[i] == '\n' )
			declaration->sig[i] = ' ';
	sig = declaration->sig;
	size_t o = 0;
	for ( size_t i = 0; sig[i]; i++ )
	{
		if ( i && (sig[i-1] == ' ' || sig[i-1] == '*') && sig[i] == ' ' )
			continue;
		declaration->sig[o++] = sig[i];
	}
	declaration->sig[o] = '\0';
	const char* name_str = sig;
	size_t parens = 0;
	for ( size_t i = 0; sig[i]; i++ )
	{
		if ( sig[i] == '(' )
			parens++;
		if ( is_identifier(sig[i]) )
		{
			if ( i && !parens && (sig[i-1] == ' ' || sig[i-1] == '*') )
				name_str = sig + i;
			if ( 2 <= i && sig[i-2] == '(' && sig[i-1] == '*' && sig[i]  )
				name_str = sig + i;
		}
		if ( sig[i] == ')' )
			parens--;
	}
	size_t name_len = 0;
	while ( is_identifier(name_str[name_len]) )
		name_len++;
	if ( !(declaration->name = strndup(name_str, name_len)) )
		abort();
	if ( parent && !(declaration->parent = strdup(parent)) )
		abort();
	if ( following_options &&
	     !(declaration->options = strdup(following_options)) )
		abort();
	declarations[declarations_used++] = declaration;
#ifdef MEANWHILE
	output_declaration(declaration, stdout);
#endif
	return declaration;
}

static struct declaration* add_declaration_mask(const char* sig, int type_mask)
{
	struct declaration* declaration =
		add_declaration_to_parent(sig, type_mask, NULL);
	parent_id = declarations_used - 1;
	return declaration;
}

static struct declaration* add_declaration(const char* sig, enum type type)
{
	return add_declaration_mask(sig, REQUIRED_TYPE(type));
}

static struct declaration* add_member(const char* sig, enum type type)
{
	const char* parent = declarations[parent_id]->sig;
	return add_declaration_to_parent(sig, REQUIRED_TYPE(type), parent);
}

static void apply_shall_define_type(enum type type)
{
	following_type = REQUIRED_TYPE(type);
	while ( in_shall_define_from < declarations_used )
	{
		struct declaration* declaration = declarations[in_shall_define_from++];
		const char* name = declaration->name;
#ifdef MEANWHILE
		printf("correction: %s: %s\n", type_names[type], name);
#endif
		declaration->type_mask = REQUIRED_TYPE(type);
		if ( type == TYPE_STRUCTURE )
		{
			free(declaration->sig);
			if ( asprintf(&declaration->sig, "struct %s", name) < 0 )
				abort();
		}
		else if ( type == TYPE_UNION )
		{
			free(declaration->sig);
			if ( asprintf(&declaration->sig, "union %s", name) < 0 )
				abort();
		}
		else if ( type == TYPE_ENUMERATION )
		{
			free(declaration->sig);
			if ( asprintf(&declaration->sig, "enum %s", name) < 0 )
				abort();
		}
	}
}

bool try_regex(const char** input, const char* regex)
{
	if ( regex[0] != '^' )
	{
		fprintf(stderr, "bad: %s\n", regex);
		abort();
	}
	regex_t re;
	if ( regcomp(&re, regex, REG_EXTENDED) )
		err(1, "regcomp: %s", regex);
	regmatch_t match;
	bool result = false;
	if ( !regexec(&re, *input, 1, &match, 0) )
	{
#ifdef DEBUG
		printf(DEBUG_COLOR "%.*s" END_COLOR "\n", match.rm_eo, *input);
		printf(DEBUG_COLOR "%s" END_COLOR "\n", regex);
#endif
		result = true;
		*input += match.rm_eo;
	}
	regfree(&re);
	return result;
}

bool try_regex_match(const char** input, const char* regex, char** match_ptr)
{
	if ( regex[0] != '^' )
	{
		printf("bad: %s", regex);
		abort();
	}
	regex_t re;
	if ( regcomp(&re, regex, REG_EXTENDED) )
		err(1, "regcomp: %s", regex);
	regmatch_t match[2];
	bool result = false;
	if ( !regexec(&re, *input, 2, match, 0) )
	{
#ifdef DEBUG
		printf(DEBUG_COLOR "%.*s" END_COLOR "\n", match[0].rm_eo, *input);
		printf(DEBUG_COLOR "%s" END_COLOR "\n", regex);
#endif
		result = true;
		free(*match_ptr);
		if ( !(*match_ptr = strndup(*input + match[1].rm_so,
		                            match[1].rm_eo - match[1].rm_so)) )
			err(1, "malloc");
		*input += match[0].rm_eo;
	}
	regfree(&re);
	return result;
}

void skip_sentence(const char** input_ptr)
{
	const char* input = *input_ptr;
#ifdef DEBUG
	const char* skipped = input;
#endif
	size_t depth = 0;
	size_t inside = 0;
	while ( *input )
	{
		if ( !strncmp(input, "&nbsp;", 6) )
		{
			input += 6;
			continue;
		}
		char c = *input++;
		if ( c == '<' && *input != '/' && strncmp(input, "img", 3) != 0 )
			depth++;
		else if ( c == '<' && *input == '/' )
			depth--;
		if ( c == '<' )
			inside++;
		if ( c == '>' )
			inside--;
		if ( !depth && !inside && (c == '.' || c == ':' || c == ';') )
			break;
	}
#ifdef DEBUG
	printf(DEBUG_COLOR "%.*s" END_COLOR "\n", (int) (input - skipped), skipped);
#endif
	*input_ptr = input;
}

void parse(const char* input, const char* path)
{
	(void) path;
	total += strlen(input);
	enum state
	{
		STATE_HEAD,
		STATE_SYNOPSIS,
		STATE_DESCRIPTION,
		STATE_OTHER,
		STATE_NAMESPACE,
	} state = STATE_HEAD;
	char* match = NULL;
	bool in_b = false;
	bool in_center = false;
	bool in_i = false;
	bool in_p = false;
	bool in_dd = false;
	bool in_dl = false;
	bool in_dt = false;
	bool in_table = false;
	bool in_tbody = false;
	bool in_td = false;
	bool in_th = false;
	bool in_tr = false;
	bool in_tt = false;
	bool in_div = false;
	bool in_pre = false;
	bool in_ul = false;
	size_t in_blockquote = 0;
	size_t in_column = 0;
	size_t want_column = 0;
	size_t type_column = 0;
	size_t header_column = 0;
	bool right_header = false;
	char* type_from_column = NULL;
	bool type_from_column_header = false;
	bool in_shall_define = false;
	bool in_following_colon = false;
	bool in_following_colon_any = false;
	bool in_following = false;
	bool in_enum_members = false;
	bool in_struct_members = false;
	bool in_union_members = false;
	bool in_definitions = false;
	bool in_functions = false;
	bool in_generic = false;
	bool in_integer_macros = false;
	bool in_type_generic = false;
	bool in_variables = false;
	bool in_enum = false;
	bool in_br_definitions = false;
	bool in_unistd_options = false;
	bool ignore_dl = false;
	bool text_inside_dd = false;
	while ( *input )
	{
		if ( isspace((unsigned char) *input) )
			input++;
		else if ( try_regex(&input, "^&nbsp;") )
			;
		else if ( try_regex(&input, "^<p>The characteristics of floating types are defined in terms of a model.*The floating-point model representation is provided for all values except FLT_EVAL_METHOD and FLT_ROUNDS.</p>") )
			;
		else if ( try_regex(&input, "^<h4 class=\"mansect\"> *<a +[^>]*> *</a> *SYNOPSIS</h4>") )
			state = STATE_SYNOPSIS;
		else if ( try_regex(&input, "^<h4 class=\"mansect\"> *<a +[^>]*> *</a> *DESCRIPTION</h4>") )
			state = STATE_DESCRIPTION;
		else if ( try_regex(&input, "^<h4> *<a +[^>]*> *</a>[^<]*The Name Space</h4>") )
			state = STATE_NAMESPACE;
		else if ( try_regex(&input, "^<h4 class=\"mansect\"> *<a +[^>]*> *</a>[^<]*</h4>") )
		{
			if ( state == STATE_DESCRIPTION || state == STATE_NAMESPACE )
				break;
			state = STATE_OTHER;
		}
		else if ( try_regex(&input, "^<h3> *<a +[^>]*> *</a>[^<]*</h3>") )
		{
			if ( state == STATE_DESCRIPTION || state == STATE_NAMESPACE )
				break;
			state = STATE_OTHER;
		}
		else if ( state != STATE_SYNOPSIS &&
		          state != STATE_DESCRIPTION &&
		          state != STATE_NAMESPACE )
		{
			if ( *input == '<' )
			{
				while ( *input && *input != '>' )
					input++;
				if ( *input == '>' )
					input++;
			}
			else
			{
				while ( *input && *input != '<' )
					input++;
			}
		}
		else if ( try_regex(&input, "^<h5> *<a +[^>]*> *</a>[^<]*</h5>") )
			in_enum_members = false, in_struct_members = false,
			in_union_members = false, in_definitions = false,
			in_functions = false, in_generic = false, in_variables = false,
			in_enum = false, in_following = false, in_following_colon = false,
			following_type = REQUIRED_TYPE(TYPE_DEFINITION);
		else if ( try_regex(&input, "^<hr>") )
			;
		else if ( try_regex(&input, "^<div class=\"box\"><em>The following sections are informative.</em></div>") )
			break;
		else if ( !in_ul && try_regex(&input, "^<ul( [^>]*)?>") )
			in_ul = true;
		else if ( in_ul && try_regex(&input, "^</ul>") )
			in_ul = false;
		else if ( !in_b && !in_generic && try_regex(&input, "^<b( [^>]*)?>") )
			in_b = true;
		else if ( in_b && !in_generic && try_regex(&input, "^</b>") )
			in_b = false;
		else if ( !in_center && try_regex(&input, "^<center( [^>]*)?>") )
			in_center = true;
		else if ( in_center && try_regex(&input, "^</center>") )
			in_center = false;
		else if ( !in_i && !in_generic && try_regex(&input, "^<i( [^>]*)?>") )
			in_i = true;
		else if ( in_i && !in_generic && try_regex(&input, "^</i>") )
			in_i = false;
		else if ( !in_p && try_regex(&input, "^<p( [^>]*)?>") )
			in_p = true;
		else if ( in_p && try_regex(&input, "^</p>") )
		{
			in_p = false, in_shall_define = false, in_following_colon = false;
			if ( in_following_colon_any )
				in_following = false;
			if ( in_br_definitions )
				in_following = false, in_following_colon = false,
				in_following_colon_any = false, in_br_definitions = false;
		}
		else if ( !in_dd && try_regex(&input, "^<dd( [^>]*)?>") )
			in_dd = true, text_inside_dd = false;
		else if ( in_dd && try_regex(&input, "^</dd>") )
			in_dd = false, text_inside_dd = false;
		else if ( !in_dl && try_regex(&input, "^<dl( [^>]*)?>") )
		{
#if defined(DEBUG) || defined(MISUNDERSTOOD)
			if ( !in_following && !ignore_dl )
				printf(WARNING_COLOR "warning: dl without recognized following type: %s" END_COLOR "\n", path);
#endif
			in_dl = true;
		}
		else if ( in_dl && try_regex(&input, "^</dl>") )
			in_dl = false, in_following = false, in_following_colon = false,
			in_following_colon_any = false, ignore_dl = false,
			following_optional = false, in_unistd_options = false;
		else if ( !in_dt && try_regex(&input, "^<dt( [^>]*)?>") )
			in_dt = true;
		else if ( in_dt && try_regex(&input, "^</dt>") )
			in_dt = false;
		else if ( !in_table && try_regex(&input, "^<table( [^>]*)?>") )
			in_table = true, want_column = 0, type_column = 0,
			header_column = 0, right_header = false;
		else if ( in_table && try_regex(&input, "^</table>") )
			in_table = false, in_following = false, in_integer_macros = false,
			in_type_generic = false;
		else if ( !in_tbody && try_regex(&input, "^<tbody( [^>]*)?>") )
			in_tbody = true;
		else if ( in_tbody && try_regex(&input, "^</tbody>") )
			in_tbody = false;
		else if ( !in_td && try_regex(&input, "^<td( [^>]*)?>") )
			in_td = true, in_column++;
		else if ( in_td && try_regex(&input, "^</td>") )
			in_td = false;
		else if ( !in_th && try_regex(&input, "^<th( [^>]*)?>") )
			in_th = true, in_column++;
		else if ( in_th && try_regex(&input, "^</th>") )
			in_th = false;
		else if ( !in_tr && try_regex(&input, "^<tr( [^>]*)?>") )
			in_tr = true, in_column = 0;
		else if ( in_tr && try_regex(&input, "^</tr>") )
		{
			if ( type_from_column )
			{
				if ( !declarations_used )
					abort();
				struct declaration* declaration =
					declarations[declarations_used - 1];
				char* sig;
				if ( asprintf(&sig, "%s %s", type_from_column,
				              declaration->name) < 0 )
					abort();
				free(declaration->sig);
				declaration->sig = sig;
				free(type_from_column);
				type_from_column = NULL;
			}
			in_tr = false;
		}
		else if ( !in_tt && try_regex(&input, "^<tt( [^>]*)?>") )
			in_tt = true;
		else if ( in_tt && try_regex(&input, "^</tt>") )
			in_tt = false;
		else if ( !in_div && try_regex(&input, "^<div( [^>]*)?>") )
			in_div = true;
		else if ( in_div && try_regex(&input, "^</div>") )
			in_div = false;
		else if ( !in_pre && try_regex(&input, "^<pre( [^>]*)?>") )
			in_pre = true;
		else if ( in_pre && try_regex(&input, "^</pre>") )
			in_pre = false, in_enum_members = false, in_struct_members = false,
			in_union_members = false, in_definitions = false,
			in_functions = false, in_generic = false, in_variables = false,
			in_enum = false, following_type = REQUIRED_TYPE(TYPE_DEFINITION);
		else if ( try_regex(&input, "^<blockquote( [^>]*)?>") )
			in_blockquote++;
		else if ( in_blockquote && try_regex(&input, "^</blockquote>") )
			in_blockquote--;
		else if ( try_regex(&input, "^<basefont size=\"[0-9]+\">") )
			;
		else if ( try_regex(&input, "^<div class=\"box\"><em>The following sections are informative.</em></div>") )
			break;
		else if ( try_regex(&input, "^Some of the functionality described on this reference page extends the ISO&nbsp;C standard\\. .* to enable the visibility of these symbols in this header\\.") )
			;
		else if ( try_regex(&input, "^(Some of the|The) functionality described on this reference page (extends|is aligned with) the ISO&nbsp;C standard\\. Any conflict between the requirements described here and the ISO&nbsp;C standard is unintentional\\. This volume of POSIX.1-.... defers to the ISO&nbsp;C standard\\.") )
			;
		else if ( try_regex(&input, "^Implementations shall not define the macro __STDC_NO_COMPLEX__.*need not provide this header nor support any of its facilities\\.") )
			;
		else if ( try_regex_match(&input, "^<sup> *\\[ *<a +href= *\"javascript:open_code\\('[^']*'\\)\"> *([^<]+) *</a> *\\] *</sup> *<img +src= *\"\\.\\./images/opt-start\\.gif\" +alt= *\"\\[Option Start\\]\" +border= *\"0\">", &match) )
		{
			//printf(OUTPUT_COLOR  "[%s]" END_COLOR "\n", match);
			if ( !header )
			{
				printf("[%s] ", match);
				if ( strcmp(match, "CX") != 0 &&
				     !(header_options = strdup(match)) )
					abort();
			}
			free(following_options);
			if ( header_options ?
			     asprintf(&following_options, "%s %s", header_options,
			              match) < 0 :
			     !(following_options = strdup(match)) )
				abort();
			if ( in_dd && !text_inside_dd )
			{
				if ( !declarations_used )
					abort();
				free(declarations[declarations_used-1]->options);
				if ( !(declarations[declarations_used-1]->options = strdup(match)) )
					abort();
#ifdef MEANWHILE
				printf("[%s] correcting previous options: %s\n", match, declarations[declarations_used-1]->name);
#endif
			}
		}
		else if ( try_regex(&input, "^<img +src= *\"\\.\\./images/opt-end\\.gif\" +alt= *\"\\[Option End\\]\" +border= *\"0\">") )
		{
			free(following_options);
			if ( header_options )
			{
				if ( !(following_options = strdup(header_options)) )
					abort();
			}
			else
				following_options = NULL;
		}
		else if ( state == STATE_SYNOPSIS &&
		          try_regex_match(&input, "^#include &lt;([^&]*\\.h)&gt;", &header) )
		{
			printf(OUTPUT_COLOR  "#include <%s>" END_COLOR "\n", header);
			// POSIX forgot to say stdio.h declares asprintf and vasprintf.
			// TODO: Only do this if a POSIX 2024 edition with this mistake.
			if ( !strcmp(header, "stdio.h") )
			{
				char* old_options = following_options;
				following_options = "CX";
				int type = OPTIONAL_TYPE(TYPE_DEFINITION) |
				           REQUIRED_TYPE(TYPE_FUNCTION);
				add_declaration_mask("int asprintf(char **restrict, const char *restrict, ...)", type);
				add_declaration_mask("int vasprintf(char **restrict, const char *restrict, va_list)", type);
				following_options = old_options;
			}
			// POSIX-1.2024 updated to C17 but forgot to add static_assert to
			// <assert.h>.
			if ( !strcmp(header, "assert.h") )
			{
				// TODO: Don't add static_assert when parsing older versions.
				add_declaration("static_assert", TYPE_DEFINITION);
			}
			// Unfortunately POSIX typo'd DL_ifno to DL_info_t and now both
			// exists. https://www.austingroupbugs.net/view.php?id=1847
			if ( !strcmp(header, "dlfcn.h") )
			{
				// TODO: Don't add Dl_info when parsing older versions.
				add_declaration("Dl_info", TYPE_TYPE);
			}
		}
		else if ( in_p && try_regex(&input, "^The <i>&lt;(langinfo|limits|regex|stdarg|stdint|termios|sys/stat|unistd|wordexp)\\.h&gt;</i> header (shall contain|defines miscellaneous|shall define macros and symbolic constants for|shall declare sets of integer types[^.]*\\.|shall define the (symbolic constants (used|needed)|structures and symbolic constants|structure of the data))") )
			skip_sentence(&input);
		else if ( in_p && try_regex(&input, "^The <i>&lt;cpio\\.h&gt;</i> header shall define the symbolic constants needed by the <i>c_mode</i> field of the <i>cpio</i> archive format, with the names and values given in the following table:") )
		{
			in_following = true;
			following_type = REQUIRED_TYPE(TYPE_SYMBOLIC_CONSTANT);
		}
		else if ( in_p && try_regex_match(&input, "^((and|It|(The|In addition, the) <i>&lt;[^&]*\\.h&gt;</i> header) (shall |may )?(also )?(define|declare|provide|contain)s? )", &match) )
		{
			in_shall_define = true;
			in_shall_define_from = declarations_used;
			following_type = REQUIRED_TYPE(TYPE_DEFINITION);
			following_optional = strstr(match, "may");
		}
		else if ( in_shall_define && in_b &&
		          try_regex_match(&input, "^([^<]+)", &match) )
		{
			if ( !strcmp(match, "size_t") )
				following_type = REQUIRED_TYPE(TYPE_TYPE);
			add_declaration_mask(match, following_type);
		}
		else if ( in_p && try_regex(&input, "^(If any of the following symbolic constants are not defined in the|If the following symbolic constants are defined in the)") )
		{
			following_optional = true;
			following_type = REQUIRED_TYPE(TYPE_SYMBOLIC_CONSTANT);
			skip_sentence(&input);
		}
		else if ( !in_shall_define && try_regex(&input, "^and the structure") )
		{
			in_shall_define = true;
			in_shall_define_from = declarations_used;
			following_type = REQUIRED_TYPE(TYPE_STRUCTURE);
		}
		else if ( in_dd && try_regex_match(&input, "^A value of t?y?p?e? ?<b>([^<]+)</b> ([^<]+)", &match) )
		{
			if ( !declarations_used )
				abort();
			struct declaration* declaration = declarations[declarations_used-1];
			free(declaration->sig);
			if ( asprintf(&declaration->sig, "%s %s", match,
			              declaration->name) < 0 )
				abort();
		}
		// NOTE: Options for definitions can be declared inside <dd>. But only
		//       if the whole definition is covered in it. There may be multiple
		//       overlapping clauses, e.g. see TMP_MAX. Also there may be a
		//       nested dd inside the options.
		else if ( in_dd && try_regex_match(&input, "^([^<]+)", &match) )
		{
			if ( in_unistd_options &&
			     (!following_options || !strcmp(following_options, "OB")) &&
			     !strstr(match, "This symbol shall always be") &&
			     !strstr(match, "This symbol shall be defined") &&
			     !strstr(match, "The use of") )
			{
				if ( !declarations_used )
					abort();
				declarations[declarations_used-1]->optional = true;
			}
			else if ( in_unistd_options )
			{
				if ( strstr(match, "The use of") )
					skip_sentence(&input);
			}
		}
		else if ( in_dt &&
		          try_regex(&input, "^Note:") )
			;
		else if ( in_dt && state == STATE_DESCRIPTION &&
		          try_regex_match(&input, "^[[{]?([^]<}]+(<i>N</i>[^]<}]+)?(\\(( *,? *(<i>)?[^<,]+(</i>)?)*\\))?)[]}]?", &match) )
		{
			if ( !ignore_dl )
			{
				char* after_n = NULL;
				for ( size_t i = 0; match[i]; i++ )
				{
					if ( !strncmp(match + i, "<i>N</i>", strlen("<i>N</i>")) )
					{
						match[i] = 0;
						after_n = match + i + strlen("<i>N</i>");
						break;
					}
					else if ( !strncmp(match + i, "<i>", 3) )
					{
						memmove(match+i, match+i+3, strlen(match+i+3)+1);
						i--;
					}
					else if ( !strncmp(match + i, "</i>", 4) )
					{
						memmove(match+i, match+i+4, strlen(match+i+4)+1);
						i--;
					}
				}
				if ( after_n )
				{
					// TODO: Some of the CX options are inaccurate for stdint.h.
					char* name;
					if ( asprintf(&name, "%s8%s", match, after_n) < 0 )
						abort();
					add_declaration_mask(name, following_type);
					free(name);
					if ( asprintf(&name, "%s16%s", match, after_n) < 0 )
						abort();
					add_declaration_mask(name, following_type);
					free(name);
					if ( asprintf(&name, "%s32%s", match, after_n) < 0 )
						abort();
					add_declaration_mask(name, following_type);
					free(name);
					if ( asprintf(&name, "%s64%s", match, after_n) < 0 )
						abort();
					add_declaration_mask(name, following_type);
					free(name);
				}
				else
					add_declaration_mask(match, following_type);
				if ( !strcmp(match, "div_t") )
				{
					add_member("int quot", TYPE_STRUCTURE_MEMBER);
					add_member("int rem", TYPE_STRUCTURE_MEMBER);
				}
				else if ( !strcmp(match, "ldiv_t") )
				{
					add_member("long quot", TYPE_STRUCTURE_MEMBER);
					add_member("long rem", TYPE_STRUCTURE_MEMBER);
				}
				else if ( !strcmp(match, "lldiv_t") )
				{
					add_member("long long quot", TYPE_STRUCTURE_MEMBER);
					add_member("long long rem", TYPE_STRUCTURE_MEMBER);
				}
				else if ( !strcmp(match, "imaxdiv_t") )
				{
					add_member("intmax_t quot", TYPE_STRUCTURE_MEMBER);
					add_member("intmax_t rem", TYPE_STRUCTURE_MEMBER);
				}
			}
		}
		else if ( in_following && !in_dl &&
		          try_regex_match(&input, "^<br> ([a-zA-Z0-9_]+)", &match) )
		{
			in_br_definitions = true;
			if ( !strcmp(match, "PTHREAD_CANCELED") )
				add_declaration_mask("void *PTHREAD_CANCELED", following_type);
			else if ( !strcmp(match, "PTHREAD_ONCE_INIT") )
				add_declaration_mask("pthread_once_t PTHREAD_ONCE_INIT",
				                     following_type);
			else if ( !strcmp(match, "WCOREDUMP") ||
			          !strcmp(match, "WEXITSTATUS") ||
			          !strcmp(match, "WIFEXITED") ||
			          !strcmp(match, "WIFSIGNALED") ||
			          !strcmp(match, "WIFSTOPPED") ||
			          !strcmp(match, "WSTOPSIG") ||
			          !strcmp(match, "WTERMSIG") )
				add_declaration_mask(match,
				                     REQUIRED_TYPE(TYPE_DEFINITION));
			else
				add_declaration_mask(match, following_type);
		}
		else if ( try_regex(&input, "^<br>") )
			;
		else if ( in_shall_define && try_regex(&input, "^the values used for <i>[^<]*</i>") )
			;
		else if ( in_shall_define && try_regex(&input, "^as an alias for <b>[^<]*</b>") )
			;
		else if ( in_shall_define && try_regex(&input, "^a special locale object descriptor used by the <a +href= *\"[^\"]*\"><i>duplocale</i>\\(\\)</a> and <a +href= *\"[^\"]*\"><i>uselocale</i>\\(\\)</a> functions") )
			;
		else if ( in_shall_define && try_regex(&input, "^, which is used in getting and setting the attributes of a message queue. Attributes are initially set when the message queue is created. An <b>mq_attr</b> structure shall have at least the following fields:") )
			in_struct_members = true;
		else if ( in_shall_define && try_regex(&input, "^, which is returned by <a +href= *\"[^\"]*\"><i>times</i>\\(\\)</a>") )
			;
		else if ( in_shall_define && try_regex(&input, "^, which shall be:</p> *<ul> *<li> *<p>Large enough to accommodate all supported protocol-specific address structures</p> *</li> *<li> *<p>Aligned at an appropriate boundary so that pointers to it can be cast as pointers to protocol-specific address structures and used to access the fields of those structures without alignment problems</p> *</li> *</ul> *<p>The <b>sockaddr_storage</b> structure") )
			;
		else if ( in_shall_define && try_regex(&input, "^each of the atomic integer types in the following table as a type that has the same representation and alignment requirements as the corresponding direct type\\.") )
		{
			following_type = REQUIRED_TYPE(TYPE_TYPE);
			in_following = true;
			in_shall_define = false;
		}
		else if ( in_shall_define && try_regex(&input, "^symbolic constants for file modes for use as values of <b>mode_t</b>") )
		{
			// fnctl.h
			add_declaration("S_IRWXU", TYPE_DEFINITION);
			add_declaration("S_IRUSR", TYPE_DEFINITION);
			add_declaration("S_IWUSR", TYPE_DEFINITION);
			add_declaration("S_IXUSR", TYPE_DEFINITION);
			add_declaration("S_IRWXG", TYPE_DEFINITION);
			add_declaration("S_IRGRP", TYPE_DEFINITION);
			add_declaration("S_IWGRP", TYPE_DEFINITION);
			add_declaration("S_IXGRP", TYPE_DEFINITION);
			add_declaration("S_IRWXO", TYPE_DEFINITION);
			add_declaration("S_IROTH", TYPE_DEFINITION);
			add_declaration("S_IWOTH", TYPE_DEFINITION);
			add_declaration("S_IXOTH", TYPE_DEFINITION);
			add_declaration("S_ISUID", TYPE_DEFINITION);
			add_declaration("S_ISGID", TYPE_DEFINITION);
			char* old_options = following_options;
			following_options = (char*) "XSI";
			add_declaration("S_ISVTX", TYPE_DEFINITION);
			following_options = old_options;
		}
		else if ( in_shall_define && try_regex(&input, "^symbolic names for <i>st_mode</i> and the file type test macros") )
		{
			// ftw.h
			add_declaration("S_IRWXU", TYPE_DEFINITION);
			add_declaration("S_IRUSR", TYPE_DEFINITION);
			add_declaration("S_IWUSR", TYPE_DEFINITION);
			add_declaration("S_IXUSR", TYPE_DEFINITION);
			add_declaration("S_IRWXG", TYPE_DEFINITION);
			add_declaration("S_IRGRP", TYPE_DEFINITION);
			add_declaration("S_IWGRP", TYPE_DEFINITION);
			add_declaration("S_IXGRP", TYPE_DEFINITION);
			add_declaration("S_IRWXO", TYPE_DEFINITION);
			add_declaration("S_IROTH", TYPE_DEFINITION);
			add_declaration("S_IWOTH", TYPE_DEFINITION);
			add_declaration("S_IXOTH", TYPE_DEFINITION);
			add_declaration("S_ISUID", TYPE_DEFINITION);
			add_declaration("S_ISGID", TYPE_DEFINITION);
			char* old_options = following_options;
			following_options = (char*) "XSI";
			add_declaration("S_ISVTX", TYPE_DEFINITION);
			following_options = old_options;
			add_declaration("S_ISBLK(m)", TYPE_DEFINITION);
			add_declaration("S_ISCHR(m)", TYPE_DEFINITION);
			add_declaration("S_ISDIR(m)", TYPE_DEFINITION);
			add_declaration("S_ISFIFO(m)", TYPE_DEFINITION);
			add_declaration("S_ISREG(m)", TYPE_DEFINITION);
			add_declaration("S_ISLNK(m)", TYPE_DEFINITION);
			add_declaration("S_ISSOCK(m)", TYPE_DEFINITION);
			add_declaration("S_TYPEISMQ(buf)", TYPE_DEFINITION);
			add_declaration("S_TYPEISSEM(buf)", TYPE_DEFINITION);
			add_declaration("S_TYPEISSHM(buf)", TYPE_DEFINITION);
			old_options = following_options;
			following_options = (char*) "XSI TYM";
			add_declaration("S_TYPEISTMO(buf)", TYPE_DEFINITION);
			following_options = old_options;
		}
		else if ( in_shall_define && try_regex(&input, "^as the function pointer type <b>[^>]+</b>") )
			apply_shall_define_type(TYPE_TYPE);
		else if ( in_shall_define && try_regex(&input, "^as the[^.:]*") )
			;
		else if ( in_shall_define && try_regex(&input, "^the structures and symbolic constants") )
			in_shall_define = false;
		else if ( in_shall_define && try_regex(&input, "^following socket types \\(see XSH <a +href= *\"[^\"]*\"><i>2.10.6 Socket Types</i></a>\\) as symbolic constants with distinct values:") )
		{
			in_following = true;
			in_definitions = true;
			in_shall_define = false;
			following_type = REQUIRED_TYPE(TYPE_SYMBOLIC_CONSTANT);
		}
		else if ( in_shall_define && try_regex(&input, "^for variables used to traverse the list\\.") )
			in_shall_define = false;
		else if ( in_shall_define && try_regex(&input, "^a declaration or definition for <i>getdate_err</i>\\. The <i>getdate_err</i> symbol shall expand to an expression of type <b>int</b>\\. It is unspecified whether <i>getdate_err</i> is a macro or an identifier declared with external linkage, and whether or not it is a modifiable lvalue\\. If a macro definition is suppressed in order to access an actual object, or a program defines an identifier with the name <i>getdate_err</i>, the behavior is undefined\\.") )
			add_declaration_to_parent("int getdate_err",
			                          REQUIRED_TYPE(TYPE_EXPRESSION), NULL);
		else if ( in_shall_define && try_regex_match(&input, "^which shall have type <b>([^<]+)</b>", &match) )
		{
			if ( !declarations_used )
				abort();
			struct declaration* declaration = declarations[declarations_used-1];
			free(declaration->sig);
			if ( asprintf(&declaration->sig, "%s %s", match,
			              declaration->name) < 0 )
				abort();
			in_shall_define = false;
			skip_sentence(&input);
		}
		else if ( in_shall_define && try_regex_match(&input, "^which shall evaluate to the same value as \\(\\(<b>void \\*</b>)\\(<b>intptr_t</b>\\)-1\\)", &match) )
		{
			if ( !declarations_used )
				abort();
			struct declaration* declaration = declarations[declarations_used-1];
			free(declaration->sig);
			if ( asprintf(&declaration->sig, "void *%s",
			              declaration->name) < 0 )
				abort();
		}
		else if ( in_shall_define && try_regex(&input, "^the ") )
			;
		else if ( in_shall_define && try_regex(&input, "^, ") )
			;
		else if ( in_shall_define && try_regex(&input, "^and ") )
			;
		else if ( in_shall_define && try_regex(&input, "^or ") )
			;
		else if ( in_shall_define && try_regex(&input, "^for ") )
			;
		else if ( in_shall_define && try_regex(&input, "^a ") )
			;
		else if ( in_shall_define && try_regex(&input, "^values ") )
			;
		else if ( in_shall_define && try_regex(&input, "^used in") )
		{
			skip_sentence(&input);
			in_shall_define = false;
		}
		else if ( in_shall_define && try_regex(&input, "^used ") )
			;
		else if ( in_shall_define && try_regex(&input, "^each ") )
			;
		else if ( in_shall_define && try_regex(&input, "^of ") )
			;
		else if ( in_shall_define && try_regex(&input, "^at ") )
			;
		else if ( in_shall_define && try_regex(&input, "^least ") )
			;
		else if ( in_shall_define && try_regex(&input, "^declarations?") )
			;
		else if ( in_shall_define && try_regex(&input, "^following atomic lock-free") )
			;
		else if ( in_shall_define && try_regex(&input, "^structure types?") )
			apply_shall_define_type(TYPE_TYPE);
		else if ( in_shall_define && try_regex(&input, "^array types?") )
			apply_shall_define_type(TYPE_TYPE);
		else if ( in_shall_define && try_regex(&input, "^(as )?(a )?(struct|structures?)") )
			apply_shall_define_type(TYPE_STRUCTURE);
		else if ( in_shall_define && try_regex(&input, "^(as a )?(un)?signed integer type") )
			apply_shall_define_type(TYPE_TYPE);
		else if ( in_shall_define && try_regex(&input, "^via <b>typedef</b>") )
			apply_shall_define_type(TYPE_TYPE);
		else if ( in_shall_define && try_regex(&input, "^unions?") )
			apply_shall_define_type(TYPE_UNION);
		else if ( in_shall_define && try_regex(&input, "^definitions?") )
			apply_shall_define_type(TYPE_DEFINITION);
		else if ( in_shall_define && try_regex(&input, "^macros?") )
			apply_shall_define_type(TYPE_DEFINITION);
		else if ( in_shall_define && try_regex(&input, "^symbolic constants?") )
			apply_shall_define_type(TYPE_SYMBOLIC_CONSTANT);
		else if ( in_shall_define && try_regex(&input, "^(types? )?(as )?(an )?(enumeration|enumerated) (data )?types?") )
			apply_shall_define_type(TYPE_TYPE);
		else if ( in_shall_define && try_regex(&input, "^enumeration constants?") )
			apply_shall_define_type(TYPE_ENUMERATION_MEMBER);
		else if ( in_shall_define && try_regex(&input, "^(as complete object )?types?( through <b>typedef</b>)?") )
			apply_shall_define_type(TYPE_TYPE);
		else if ( in_shall_define && try_regex(&input, "^((which|that|which shall include the scheduling parameters required for implementation of each supported scheduling policy. This structure) )?shall include (at least ?)the following members?[.:]") )
		{
			in_struct_members = true;
			in_shall_define = false;
		}
		else if ( in_struct_members && in_pre && in_tt &&
		          (try_regex_match(&input, "^([^<]+\\[</tt><i>[^>]*</i><tt>\\])", &match) ||
		           try_regex_match(&input, "^([^<\n]+)", &match)) )
		{
			for ( size_t i = 0; match[i]; i++ )
			{
				if ( !strncmp(match + i, "</tt><i>", 8) ||
				     !strncmp(match + i, "</i><tt>", 8) )
				{
					memmove(match+i, match+i+8, strlen(match+i+8)+1);
					i--;
				}
			}
			add_member(match, TYPE_STRUCTURE_MEMBER);
		}
		else if ( in_struct_members && in_pre && !in_tt &&
		          try_regex_match(&input, "^([^<]+)", &match) )
		{
			//printf(OUTPUT_COLOR  "structure member description: %s" END_COLOR "\n", match);
		}
		else if ( in_union_members && in_pre && in_tt &&
		          try_regex_match(&input, "^([^<\n]+)", &match) )
			add_member(match, TYPE_UNION_MEMBER);
		else if ( in_union_members && in_pre && !in_tt &&
		          try_regex_match(&input, "^([^<]+)", &match) )
		{
			//printf(OUTPUT_COLOR  "union member description: %s" END_COLOR "\n", match);
		}
		else if ( in_following && in_pre && (!in_tt || in_enum_members) &&
		          try_regex_match(&input, "^([a-zA-Z_][a-zA-Z_0-9]*)", &match) )
			add_declaration_mask(match, following_type);
		else if ( in_following && in_p && !in_pre && !in_tt &&
		          try_regex_match(&input, "^(PTHREAD_NULL)", &match) )
			add_declaration_mask("pthread_t PTHREAD_NULL", REQUIRED_TYPE(TYPE_SYMBOLIC_CONSTANT));
		else if ( in_following && !in_p && in_pre && in_tt &&
		          try_regex(&input, "^enum \\{ FIND, ENTER \\} ACTION;") )
		{
			add_declaration_to_parent("FIND", REQUIRED_TYPE(TYPE_ENUMERATION_MEMBER), "ACTION");
			add_declaration_to_parent("ENTER", REQUIRED_TYPE(TYPE_ENUMERATION_MEMBER), "ACTION");
		}
		else if ( in_following && !in_p && in_pre && in_tt &&
		          try_regex(&input, "^enum \\{ preorder, postorder, endorder, leaf \\} VISIT;") )
		{
			add_declaration_to_parent("preorder", REQUIRED_TYPE(TYPE_ENUMERATION_MEMBER), "VISIT");
			add_declaration_to_parent("postorder", REQUIRED_TYPE(TYPE_ENUMERATION_MEMBER), "VISIT");
			add_declaration_to_parent("endorder", REQUIRED_TYPE(TYPE_ENUMERATION_MEMBER), "VISIT");
			add_declaration_to_parent("leaf", REQUIRED_TYPE(TYPE_ENUMERATION_MEMBER), "VISIT");
		}
		else if ( in_shall_define &&
		          try_regex(&input, "^\\(?as +(described|defined) +in +<a( [^>]*)?> *<i> *(&lt;)?\\)? *") )
		{
			if ( !try_regex_match(&input, "^([^<& ]+)", &match) )
				abort();
			// TODO: This is kinda significant for structures/unions/enums, as
			//       we should check if all members are also declared, or if
			//       an incomplete type is used.
			//printf(OUTPUT_COLOR  "as described in: %s" END_COLOR "\n", match);
			if ( !try_regex(&input, "^ *(&gt;)? *</i> *</a>( +header)?\\)?") )
				abort();
		}
		else if ( in_shall_define && try_regex(&input, "^\\.") )
			in_shall_define = false;
		else if ( in_shall_define && try_regex(&input, "^which shall include (at least )?the following members:") )
			in_struct_members = true;
		else if ( in_shall_define && try_regex(&input, "^at least the") )
			;
		// TODO: The ISO&nbsp;C standard only requires the signal names SIGABRT, SIGFPE, SIGILL, SIGINT, SIGSEGV, and SIGTERM to be defined.
		// TODO: The ISO&nbsp;C standard only requires the symbols [EDOM], [EILSEQ], and [ERANGE] to be defined.
		else if ( in_shall_define && try_regex(&input, "^which shall expand to a modifiable lvalue of type <b>int</b> and thread local storage duration. If the macro definition is suppressed in order to access an actual object, or a program defines an identifier with the name <i>errno</i>, the behavior is undefined.") )
			;
		else if ( in_shall_define && try_regex(&input, "^describing a file lock. It shall include the following members:") )
			in_struct_members = true;
		// TODO: Refactor parsing 'following' as its own word.
		else if ( in_shall_define && try_regex_match(&input, "^((following (as )?(eleven )?(values?|macros?|symbolic constants?|compile-time constant expressions?)|through type definitions as follows|following as described))", &match) )
		{
			in_following = true;
			if ( strstr(match, "symbolic constant") )
			{
				following_type = REQUIRED_TYPE(TYPE_SYMBOLIC_CONSTANT);
				in_definitions = true;
			}
			else if ( strstr(match, "compile-time constant expressions") )
			{
				following_type = REQUIRED_TYPE(TYPE_SYMBOLIC_CONSTANT);
				in_definitions = true;
			}
			else if ( strstr(input, "through type definitions as follows") )
			{
				following_type = REQUIRED_TYPE(TYPE_ENUMERATION);
				in_enum = true;
			}
			else
			{
				following_type = REQUIRED_TYPE(TYPE_DEFINITION);
				in_definitions = true;
			}
			in_shall_define = false;
			// There's a suitable for #if requirement after the c_cc table.
			if ( try_regex(&input, "^ for use as subscripts for the array <i>c_cc</i>") )
				following_type = REQUIRED_TYPE(TYPE_DEFINITION);
			else if ( try_regex(&input, "^ if and only if the implementation supports") )
				following_optional = true;
			else
				following_optional = false;
#ifdef DEBUG
			const char* skipped = input;
#endif
			size_t depth = 0;
			size_t inside = 0;
			in_following_colon = false;
			while ( *input )
			{
				char c = *input++;
				if ( c == '<' && *input != '/' )
					depth++;
				else if ( c == '<' && *input == '/' )
					depth--;
				if ( c == '<' )
					inside++;
				if ( c == '>' )
					inside--;
				if ( !depth && !inside )
				{
					in_following_colon = c == ':';
					if ( c == '.' || c == ':' )
						break;
				}
			}
#ifdef DEBUG
			printf(DEBUG_COLOR "%.*s" END_COLOR "\n", (int) (input - skipped), skipped);
#endif
		}
		else if ( in_shall_define && try_regex(&input, "^following (types?|data types?)[^:.]*:") )
		{
			following_type = REQUIRED_TYPE(TYPE_TYPE);
			in_following = true;
		}
		else if ( in_shall_define && try_regex(&input, "^(following as|following external) variables?:") )
		{
			following_type = REQUIRED_TYPE(TYPE_EXTERNAL);
			in_variables = true;
		}
		else if ( in_shall_define && try_regex(&input, "^whose enumerators shall include at least the following:") )
		{
			following_type = REQUIRED_TYPE(TYPE_ENUMERATION_MEMBER);
			in_following = true;
			in_enum_members = true;
		}
		else if ( in_shall_define && try_regex(&input, "^following generic functions") )
		{
			skip_sentence(&input);
			following_type = REQUIRED_TYPE(TYPE_GENERIC);
			in_generic = true;
		}
		else if ( in_shall_define && try_regex(&input, "^representing a locale object") )
			;
		else if ( try_regex(&input, "^(If defined, (its value|they)|The values?) ((shall be (distinct|unique|bitwise-distinct))?( and )?shall be|shall have values) suitable for use in <b>#if</b> preprocessing directives[.,:]") )
		{
			//printf(OUTPUT_COLOR  "usable in #if" END_COLOR "\n");
			following_type = REQUIRED_TYPE(TYPE_DEFINITION);
		}
		else if ( try_regex(&input, "^The values? (shall be (distinct|unique|bitwise-distinct))[.,:]") )
			;
		else if ( in_shall_define && try_regex(&input, "^(with|that can|which|in the) ") )
		{
			skip_sentence(&input);
			in_shall_define = false;
		}
		else if ( (in_shall_define || in_following_colon) &&
		          (try_regex_match(&input, "^<a +href= *\"[^\"]*\"><i>([^<]*)</i>\\(\\)</a>", &match) ||
		           try_regex_match(&input, "^[[{]?([a-zA-Z0-9_]+)[]}]?", &match)) )
		{
			add_declaration_mask(match, following_type);
			if ( in_following_colon )
				in_following_colon_any = true;
		}
		else if ( try_regex_match(&input, "^The <b>([^>]+)</b> union shall be defined as:", &match) )
		{
			add_declaration(match, TYPE_UNION);
			in_following = true;
			in_union_members = true;
		}
		else if ( try_regex_match(&input, "^The following (shall|may) be declared as (a function|functions) and may also be defined as (a macro|macros)[.:]", &match) )
		{
			following_type = (!strcmp(match, "shall") ?
			                  REQUIRED_TYPE(TYPE_FUNCTION) :
			                  OPTIONAL_TYPE(TYPE_FUNCTION)) |
			                 OPTIONAL_TYPE(TYPE_DEFINITION);
			in_functions = true;
		}
		else if ( try_regex(&input, "^(A function prototype|Function prototypes) shall be provided\\.") )
			;
		else if ( try_regex(&input, "^The following (shall|may) be declared as (a function|functions), (or )?defined as (a macro|macros), or both[.:]") )
		{
			following_type = OPTIONAL_TYPE(TYPE_FUNCTION) |
			                 OPTIONAL_TYPE(TYPE_DEFINITION);
			in_functions = true;
		}
		else if ( try_regex(&input, "^The following external variables? shall be defined[.:]") )
		{
			following_type = REQUIRED_TYPE(TYPE_EXTERNAL);
			in_variables = true;
		}
		else if ( try_regex(&input, "^The following shall be defined as (a macro|macros)[.:]") )
		{
			following_type = REQUIRED_TYPE(TYPE_DEFINITION);
			in_definitions = true;
		}
		else if ( try_regex_match(&input, "^The following macro expands to an integer constant expression having the value specified by its argument and the type <b>[^<]+</b>: ([a-zA-Z0-9_]+)", &match) )
		{
			add_declaration(match, TYPE_DEFINITION);
		}
		else if ( try_regex_match(&input, "^The following (optional )?macros?", &match) )
		{
			following_type = REQUIRED_TYPE(TYPE_DEFINITION);
			following_optional = match && !strcmp(match, "optional ");
			in_following = true;
			skip_sentence(&input);
		}
		else if ( try_regex(&input, "^If functions are declared, function prototypes shall be provided\\.") )
			;
		else if ( try_regex(&input, "^Function prototypes shall be provided for use with ISO&nbsp;C standard compilers\\.") )
			;
		else if ( in_pre && in_generic && try_regex_match(&input, "^([^;\n]+)[;\n]", &match) )
		{
			in_b = false;
			in_i = false;
			in_tt = true;
			size_t o = 0;
			for ( size_t i = 0; match[i]; i++ )
			{
				if ( !strncmp(match + i, "<tt>", strlen("<tt>")) )
					i += strlen("<tt>") - 1;
				else if ( !strncmp(match + i, "</tt>", strlen("</tt>")) )
					i += strlen("</tt>") - 1;
				else if ( !strncmp(match + i, "<b>", strlen("<b>")) )
					i += strlen("<b>") - 1;
				else if ( !strncmp(match + i, "</b>", strlen("</b>")) )
					i += strlen("</b>") - 1;
				else if ( !strncmp(match + i, "<i>", strlen("<i>")) )
					i += strlen("<i>") - 1, match[o++] = '<';
				else if ( !strncmp(match + i, "</i>", strlen("</i>")) )
					i += strlen("</i>") - 1, match[o++] = '>';
				else
					match[o++] = match[i];
			}
			match[o] = '\0';
			add_declaration_mask(match, following_type);
		}
		else if ( in_pre && in_tt && in_functions && try_regex_match(&input, "^([^<;]+)[;\n]", &match) )
			add_declaration_mask(match, following_type);
		else if ( in_pre && !in_tt && in_functions && try_regex_match(&input, "^<a +href= *\"[^\"]*\"><i>([^<(]+)</i>\\(\\)</a>", &match) )
		{
			if ( !strcmp(match, "pthread_cleanup_pop") )
				add_declaration_mask("void pthread_cleanup_pop(int)", following_type);
			else if ( !strcmp(match, "pthread_cleanup_push") )
				add_declaration_mask("void pthread_cleanup_push(void (*)(void*), void *)", following_type);
			else
				add_declaration_mask(match, following_type);
		}
		else if ( in_pre && in_tt && in_definitions && try_regex_match(&input, "^([^<; ]+) *\"[^\"]*\"", &match) )
		{
			char* sig;
			if ( asprintf(&sig, "const char *%s", match) < 0 )
				abort();
			add_declaration_mask(sig, following_type);
			free(sig);
		}
		else if ( in_pre && in_tt && in_definitions && try_regex_match(&input, "^([^<;]+)[;\n]", &match) )
			add_declaration_mask(match, following_type);
		else if ( in_pre && !in_tt && in_definitions && try_regex_match(&input, "^<a +href= *\"[^\"]*\"><i>([^<(]+)</i>\\(\\)</a>", &match) )
			add_declaration_mask(match, following_type);
		else if ( in_pre && in_tt && in_enum && try_regex_match(&input, "^([^<;]+);", &match) )
			add_declaration(match, TYPE_ENUMERATION);
		else if ( in_pre && in_tt && in_variables && try_regex_match(&input, "^([^<;]+)[;\n]", &match) )
		{
			if ( !strcmp(match, "extern int    opterr, optind, optopt") )
			{
				add_declaration("extern int opterr", TYPE_EXTERNAL);
				add_declaration("extern int optind", TYPE_EXTERNAL);
				add_declaration("extern int optopt", TYPE_EXTERNAL);
			}
			else
				add_declaration(match, TYPE_EXTERNAL);
		}
		else if ( try_regex(&input, "^The <a +href= *\"[^\"]*\"><i>([^<(]+)</i>\\(\\)</a> macros for (un)?signed integers are:") )
			in_integer_macros = true;
		else if ( in_th && try_regex_match(&input, "^([^<]+)", &match) )
		{
			if ( !strcmp(match, "Type") ||
			     !strcmp(match, "Initializer for Type") ||
			     (!strcmp(match, "Value") && !strcmp(header, "tar.h")) )
					type_column = in_column;
			if ( !strcmp(match, "Double") )
				type_from_column_header = true;
			if ( !strcmp(match, "Name") ||
			     !strcmp(match, "Identifier") ||
			     !strcmp(match, "Constant") ||
			     !strcmp(match, "Double") ||
			     !strcmp(match, "Long Double") ||
			     !strcmp(match, "Signal") ||
			     !strcmp(match, "Atomic type name") ||
			     !strcmp(match, "Canonical Mode") ||
			     !strcmp(match, "Non-Canonical Mode") ||
			     !strcmp(match, "Type-Generic Macro") )
				want_column |= 1 << in_column;
			if ( !strcmp(match, "Code") ) // Ignore Signal column.
				want_column = 1 << in_column;
			if ( !strcmp(match, "Member") ) // Ignore this table.
				want_column = 0;
			if ( !strcmp(match, "Header") )
				header_column = in_column;
			if ( !strcmp(match, "Prefix") )
				want_column |= 1 << in_column;
			if ( !strcmp(match, "Suffix") )
				want_column |= 1 << in_column;
			// Ignore __STDC_WANT_LIB_EXT1__ table.
			if ( want_column && !strcmp(match, "Complete Name") )
				want_column |= 1 << in_column;
		}
		else if ( in_td && in_integer_macros &&
		          try_regex_match(&input, "^([^<]+(<i>N</i>)?)", &match) )
		{
			size_t offset = strcspn(match, "<");
			if ( match[offset] == '<' )
			{
				strcpy(match + offset, "8");
				add_declaration(match, TYPE_DEFINITION);
				strcpy(match + offset, "16");
				add_declaration(match, TYPE_DEFINITION);
				strcpy(match + offset, "32");
				add_declaration(match, TYPE_DEFINITION);
				strcpy(match + offset, "64");
				add_declaration(match, TYPE_DEFINITION);
			}
			else
				add_declaration(match, TYPE_DEFINITION);
		}
		else if ( state == STATE_DESCRIPTION && in_td && !in_integer_macros &&
		          try_regex_match(&input, "^([^<]+)", &match) )
		{
			if ( state == STATE_DESCRIPTION &&
			     strcmp(match, "()") != 0 &&
			     want_column & (1 << in_column) &&
			     (!declarations_used ||
			      strcmp(declarations[declarations_used-1]->name, match) != 0) )
			{
				if ( type_from_column_header )
				{
					char* sig;
					if ( asprintf(&sig, "%s %s",
					              in_column == 1 ? "double" : "long double",
					              match) < 0 )
						abort();
					add_declaration_mask(sig, following_type);
					free(sig);
				}
				else
					add_declaration_mask(match, following_type);
			}
			else if ( in_column == type_column )
			{
				free(type_from_column);
				const char* type = match;
				if ( match[0] == '"' )
					type = "char *";
				else if ( match[0] == '\'' )
					type = "char";
				else if ( isdigit((unsigned char) match[0]) )
					type = "int";
				if ( !(type_from_column = strdup(type)) )
					abort();
			}
		}
		else if ( state == STATE_NAMESPACE && in_td &&
		          in_column == header_column &&
		          try_regex_match(&input, "^&lt;([^&]*)&gt;", &match) )
			right_header = header && !strcmp(header, match);
		else if ( state == STATE_NAMESPACE && in_td &&
		          in_column == header_column &&
		          try_regex(&input, "^ANY header") )
			right_header = true;
		else if ( state == STATE_NAMESPACE && in_td &&
		          want_column & (1 << in_column) &&
		          try_regex_match(&input, "^([^<]+)", &match) )
		{
			if ( right_header )
			{
				size_t i = 0;
				while ( match[i] )
				{
					if ( match[i] == ' ' || match[i] == ',' )
					{
						i++;
						continue;
					}
					// TODO: termios.h special cases.
					if ( !strcmp(match + i, "(See below.)") )
						break;
					size_t l = strcspn(match + i, ", ");
					char* part = strndup(match + i, l);
					if ( !part )
						abort();
					char* pattern = NULL;
					if ( (in_column == 2 &&
					      asprintf(&pattern, "^%s", part) < 0) ||
					     (in_column == 3 &&
					      asprintf(&pattern, "%s$", part) < 0) ||
					     (in_column == 4 &&
					      asprintf(&pattern, "^%s$", part) < 0) )
						abort();
					// TODO: Differentiate symbol reserved and macro reserved.
					add_declaration(pattern, TYPE_NAMESPACE);
					free(pattern);
					free(part);
					i += l;
				}
			}
		}
		else if ( try_regex(&input, "^For each unsuffixed function in the +<a [^>]*><i>&lt;math\\.h&gt;</i></a> header") )
		{
			skip_sentence(&input);
			skip_sentence(&input);
			in_type_generic = true;
		}
		else if ( try_regex(&input, "^For each unsuffixed function in the +<a [^>]*><i>&lt;complex\\.h&gt;</i></a> header") )
		{
			skip_sentence(&input);
			skip_sentence(&input);
			in_following = true;
			in_functions = true;
			following_type = REQUIRED_TYPE(TYPE_DEFINITION);
		}
		else if ( in_td && in_type_generic &&
		          try_regex_match(&input, "^<a +href= *\"[^\"]*\"><i>([^<]+)</i>\\(\\)</a>", &match) )
			add_declaration(match, TYPE_DEFINITION);
		else if ( try_regex_match(&input, "^The tag <b>([^<]+)</b> shall be declared as naming an incomplete structure type", &match) )
		{
			add_declaration(match, TYPE_STRUCTURE);
			declarations[declarations_used-1]->incomplete = true;
			if ( try_regex_match(&input, "^, the contents of which are described in the +<a [^>]*><i>&lt;([^&]*)&gt;</i></a> header\\.", &match) )
			{
					//printf(OUTPUT_COLOR  "as described in: %s" END_COLOR "\n", match);
			}
		}
		else if ( try_regex_match(&input, "^The type <b>([^>]+)</b> shall be defined as an enumeration type whose possible values shall include at least the following:", &match) )
		{
			add_declaration(match, TYPE_TYPE);
			while ( try_regex_match(&input, "^ *([a-zA-Z0-9_]+)", &match) )
				add_member(match, TYPE_ENUMERATION_MEMBER);
		}
		else if ( try_regex_match(&input, "^The type <b>([^>]+)</b> shall be defined to be the same type", &match) )
		{
			add_declaration(match, TYPE_TYPE);
			skip_sentence(&input);
		}
		else if ( try_regex_match(&input, "^The <i>([^>]+)</i> symbol shall expand to an expression of type ", &match) )
		{
			char* type = NULL;
			if ( !try_regex_match(&input, "^<b>([^>]+)</b>.", &type) )
				abort();
			char* sig;
			if ( asprintf(&sig, "%s %s", type, match) < 0 )
				abort();
			add_declaration(sig, TYPE_EXPRESSION);
			free(sig);
			free(type);
		}
		else if ( try_regex_match(&input, "^The <i>([^>]+)</i>\\(\\) macro", &match) )
		{
			add_declaration(match, TYPE_DEFINITION);
			skip_sentence(&input);
		}
		else if ( try_regex(&input, "^The macro <i>INTN_C</i>\\([^)]*\\)") )
		{
			add_declaration("INT8_C", TYPE_DEFINITION);
			add_declaration("INT16_C", TYPE_DEFINITION);
			add_declaration("INT32_C", TYPE_DEFINITION);
			add_declaration("INT64_C", TYPE_DEFINITION);
			skip_sentence(&input);
		}
		else if ( try_regex(&input, "^The macro <i>UINTN_C</i>\\([^)]*\\)") )
		{
			add_declaration("UINT8_C", TYPE_DEFINITION);
			add_declaration("UINT16_C", TYPE_DEFINITION);
			add_declaration("UINT32_C", TYPE_DEFINITION);
			add_declaration("UINT64_C", TYPE_DEFINITION);
			skip_sentence(&input);
		}
		else if ( in_p &&
		          (try_regex_match(&input, "^Inclusion of the <i>&lt;[^&]*\\.h&gt;</i> header (may|shall)( also)? make( visible)?( all)? symbols (from|defined in)( the)?( headers?)?", &match) ||
		           try_regex_match(&input, "^Inclusion of <i>&lt;[^&]*\\.h&gt;</i> (may|shall)( also)? make( visible)?( all)? symbols (from|defined in)( the)?( headers?)?", &match) ||
		           try_regex_match(&input, "^In addition, the <i>&lt;[^&]*\\.h&gt;</i> header (may|shall) include the ", &match) ||
		           try_regex_match(&input, "^The <i>&lt;[^&]*\\.h&gt;</i> header (may|shall) include the ", &match)) )
		{
			following_optional = !strcmp(match, "may");
#ifdef DEBUG
			const char* skipped = input;
#endif
			while ( *input )
			{
				if ( *input == ' ' || *input == ',' )
					input++;
				else if ( *input == '.' )
				{
					input++;
					break;
				}
				else if ( try_regex_match(&input, "^<a +[^>]*> *<i> *&lt; *([^&]*\\.h) *&gt; *</i> *</a>( +headers?)?( +visible)?", &match) )
					add_declaration(match, TYPE_INCLUDE);
				else if ( try_regex(&input, "^and")  )
					;
				else if ( try_regex(&input, "^headers?")  )
					;
				else if ( try_regex(&input, "^shall define several type-generic macros")  )
					;
				else
					break;
			}
			following_optional = false;
#ifdef DEBUG
			printf(DEBUG_COLOR "%.*s" END_COLOR "\n", (int) (input - skipped), skipped);
#endif
		}
		else if ( try_regex(&input, "^The following symbolic constants, if defined in <i>&lt;unistd\\.h&gt;</i>.*for further information about the conformance requirements of these three categories of support\\.") )
		{
			following_type = REQUIRED_TYPE(TYPE_DEFINITION); // suitable for #if
			in_following = true;
			in_unistd_options = true;
		}
		else if ( try_regex(&input, "^In addition, a macro to set the bits for all categories set shall be defined:") )
		{
			following_type = REQUIRED_TYPE(TYPE_DEFINITION);
			in_following = true;
			in_following_colon = true;
			in_following_colon_any = false;
		}
		else if ( try_regex(&input, "^(The|If an implementation provides integer types with width 64 that meet these requirements, then the) following types are required:( *</p> *<p>)?") )
		{
			following_type = REQUIRED_TYPE(TYPE_TYPE);
			in_following = true;
			in_following_colon = true;
			in_following_colon_any = false;
		}
		else if ( try_regex(&input, "^The following type designates ") )
		{
			if ( try_regex(&input, "^(a signed|an unsigned) integer type with the property") )
			{
				// intptr_t/uintptr_t are optional unless XSI.
				free(following_options);
				if ( !(following_options = strdup("XSI")) )
					abort();
			}
			skip_sentence(&input);
			fflush(stdout);
			following_type = REQUIRED_TYPE(TYPE_TYPE);
			in_following = true;
			in_following_colon = true;
			in_following_colon_any = false;
		}
		else if ( try_regex(&input, "^The <b>sched_param</b> structure defined in <i>&lt;sched.h&gt;</i> shall include the following members in addition to those specified above:") )
		{
			in_following = true;
			in_struct_members = true;
		}
		else if ( try_regex(&input, "^The default actions are as follows:") )
			ignore_dl = true;
		else if ( try_regex(&input, "^The following symbolic constants are reserved for compatibility with Issue [0-9]+:") )
		{
			following_type = REQUIRED_TYPE(TYPE_SYMBOLIC_CONSTANT);
			in_following = true;
		}
		else if ( try_regex(&input, "^All macros and symbolic constants defined in this header shall be suitable for use in <b>#if</b> preprocessing directives.") )
			following_actually_definitions = true;
		else if ( try_regex(&input, "^A definition of one of the symbolic constants in the following list shall be omitted from <i>&lt;limits.h&gt;</i> on specific implementations where the corresponding value is equal to or greater than the stated minimum, but is unspecified.") )
		{
			following_type = REQUIRED_TYPE(TYPE_SYMBOLIC_CONSTANT);
			in_following = true;
			following_optional = true;
		}
		else if ( try_regex(&input, "^A definition of one of the symbolic constants in the following list shall be omitted from the <i>&lt;limits.h&gt;</i> header on specific implementations where the corresponding value is equal to or greater than the stated minimum, but where the value can vary depending on the file to which it is applied.") )
		{
			following_type = REQUIRED_TYPE(TYPE_SYMBOLIC_CONSTANT);
			in_following = true;
			following_optional = true;
		}
		else if ( try_regex(&input, "^An application should assume that the value of the symbolic constant defined by <i>&lt;limits.h&gt;</i> in a specific implementation is the minimum that pertains whenever the application is run under that implementation.") )
		{
			following_type = REQUIRED_TYPE(TYPE_SYMBOLIC_CONSTANT);
			in_following = true;
		}
		else if ( try_regex(&input, "^Four scheduling policies are defined; others may be defined by the implementation. The four standard policies are indicated by the values of the following symbolic constants:") )
		{
			following_type = REQUIRED_TYPE(TYPE_SYMBOLIC_CONSTANT);
			in_following = true;
		}
		else if ( try_regex(&input, "^The macros imaginary and _Imaginary_I shall be defined if and only if the implementation supports imaginary types.") )
		{
			// TODO: Technically they are optional, unless the MXC option.
			for ( size_t i = 0; i < declarations_used; i++ )
			{
				if ( !strcmp(declarations[i]->name, "imaginary") ||
				     !strcmp(declarations[i]->name, "_Imaginary_I") )
					declarations[i]->optional = true;
			}
		}
		else if ( try_regex_match(&input, "^The rounding mode for floating-point addition is characterized by the implementation-defined value of (FLT_ROUNDS):", &match) )
		{
			add_declaration(match, TYPE_DEFINITION);
			ignore_dl = true;
		}
		else if ( try_regex_match(&input, "^The use of evaluation formats is characterized by the implementation-defined value of (FLT_EVAL_METHOD):", &match) )
		{
			add_declaration(match, TYPE_DEFINITION);
			ignore_dl = true;
		}
		else if ( try_regex(&input, "^The presence or absence of subnormal numbers is characterized by the implementation-defined values of FLT_HAS_SUBNORM, DBL_HAS_SUBNORM, and LDBL_HAS_SUBNORM:") )
		{
			add_declaration("FLT_HAS_SUBNORM", TYPE_DEFINITION);
			add_declaration("DBL_HAS_SUBNORM", TYPE_DEFINITION);
			add_declaration("LDBL_HAS_SUBNORM", TYPE_DEFINITION);
			ignore_dl = true;
		}
		else if ( try_regex(&input, "^For compatibility with earlier versions of this standard, the <i>st_atime</i> macro shall be defined with the value <i>st_atim.tv_sec</i>. Similarly, <i>st_ctime</i> and <i>st_mtime</i> shall be defined as macros with the values <i>st_ctim.tv_sec</i> and <i>st_mtim.tv_sec</i>, respectively.") )
		{
			add_declaration("st_atime", TYPE_DEFINITION);
			add_declaration("st_ctime", TYPE_DEFINITION);
			add_declaration("st_mtime", TYPE_DEFINITION);
		}
		else if ( try_regex(&input, "^The <a +href= *\"../functions/htonl.html\"><i>htonl</i>\\(\\)</a>, <a +href= *\"../functions/htons.html\"><i>htons</i>\\(\\)</a>, <a href= *\"../functions/ntohl.html\"><i>ntohl</i>\\(\\)</a>, and <a +href= *\"../functions/ntohs.html\"><i>ntohs</i>\\(\\)</a> functions shall be available as described in <a +href= *\"../basedefs/arpa_inet.h.html\"><i>&lt;arpa/inet.h&gt;</i></a>.") )
		{
			int mask = OPTIONAL_TYPE(TYPE_DEFINITION) |
			           OPTIONAL_TYPE(TYPE_FUNCTION);
			add_declaration_mask("uint32_t htonl(uint32_t)", mask);
			add_declaration_mask("uint16_t htons(uint16_t)", mask);
			add_declaration_mask("uint32_t ntohl(uint32_t)", mask);
			add_declaration_mask("uint16_t ntohs(uint16_t)", mask);
		}
		else if ( (state == STATE_DESCRIPTION || state == STATE_NAMESPACE) &&
		          !in_dd )
		{
			if ( in_shall_define )
			{
#if defined(DEBUG) || defined(MISUNDERSTOOD)
				printf("/* shall define ??? */");
#endif
				in_shall_define = false;
			}
			const char* old_input = input;
#if defined(DEBUG) || defined(MISUNDERSTOOD)
			printf("\t// ");
#endif
			if ( *input != '\n' )
				input++;
			while ( *input != '\n' && *input != '<' && *input != '.' )
				input++;
			if ( *input == '.' )
				input++;
#if defined(DEBUG) || defined(MISUNDERSTOOD)
			fwrite(old_input, input - old_input, 1, stdout);
#endif
			if ( *input == '\n' )
				input++;
#if defined(DEBUG) || defined(MISUNDERSTOOD)
			printf("\n");
#endif
			skipped += input - old_input;
		}
		else
		{
			if ( in_dd )
				text_inside_dd = true;
			if ( *input == '<' )
				input++;
			while ( *input && *input != '<' )
				input++;
		}
	}
	free(match);
}

void generate(void)
{
#ifndef MEANWHILE
	for ( size_t i = 0; i < declarations_used; i++ )
		output_declaration(declarations[i], stdout);
#endif
	char* prefix = strdup(header);
	if ( !prefix )
		abort();
	prefix[strlen(header) - 2] = '\0';
	for ( size_t i = 0; prefix[i]; i++ )
		if ( prefix[i] == '/' )
			prefix[i] = '_';
	if ( mkdir("../namespace", 0777) < 0 && errno != EEXIST )
		err(1, "../namespace");
	for ( int xsi = 0; xsi < 2; xsi++ )
	{
		bool actually_xsi = false;
		if ( header_options && strcmp(header_options, "CX") != 0 )
			actually_xsi = !strcmp(header_options, "XSI") ||
			               strstr(header_options, "XSI ");
		if ( actually_xsi )
			continue;
		const char* options = actually_xsi || !xsi ? header_options : "XSI";
		const char* suffix = xsi ? "-xsi" : "";
		char* namespace_test;
		if ( asprintf(&namespace_test, "../namespace/%s%s.c",
		              prefix, suffix) < 0 )
			abort();
		FILE* fp = fopen(namespace_test, "w");
		if ( !fp )
			err(1, "%s", namespace_test);
		if ( options )
			fprintf(fp, "/*[%s]*/\n", options);
		if ( xsi )
		{
			fprintf(fp, "#if 202405L <= _POSIX_C_SOURCE\n");
			fprintf(fp, "#define _XOPEN_SOURCE 800\n");
			fprintf(fp, "#elif 200809L <= _POSIX_C_SOURCE\n");
			fprintf(fp, "#define _XOPEN_SOURCE 700\n");
			fprintf(fp, "#endif\n");
		}
		fprintf(fp, "#include <%s>\n", header);
		if ( ferror(fp) || fflush(fp) == EOF )
			err(1, "write: %s", namespace_test);
		fclose(fp);
		free(namespace_test);
	}
	char* include_prefix;
	if ( asprintf(&include_prefix, "../include/%s", prefix) < 0 )
		abort();
	mkdir(include_prefix, 0777);
	for ( size_t i = 0; i < declarations_used; i++ )
	{
		const struct declaration* declaration = declarations[i];
		if ( declaration->type_mask & (OPTIONAL_TYPE(TYPE_NAMESPACE) |
		                               REQUIRED_TYPE(TYPE_NAMESPACE)) )
			continue;
		if ( declaration->type_mask & (OPTIONAL_TYPE(TYPE_INCLUDE) |
		                               REQUIRED_TYPE(TYPE_INCLUDE)) )
		{
			// TODO: If mandatory inclusion, include that API and generate tests
			//       for everything it declares too.
			//fprintf(stderr, "include: %s\n", declaration->name);
			continue;
		}
		const char* unique = declaration->name;
		if ( declaration->type_mask == REQUIRED_TYPE(TYPE_STRUCTURE) ||
		     declaration->type_mask == REQUIRED_TYPE(TYPE_UNION) ||
		     declaration->type_mask == REQUIRED_TYPE(TYPE_ENUMERATION) )
			unique = declaration->sig;
		// TODO: Case sensitive collision of nan and NAN
		char* path;
		if ( (declaration->parent ?
		      asprintf(&path, "%s/%s-%s.c", include_prefix, declaration->parent,
		               unique) :
		      asprintf(&path, "%s/%s.c", include_prefix, unique) ) < 0 )
			abort();
		for ( size_t i = 0; path[i]; i++ )
			if ( path[i] == ' ' || path[i] == '{' || path[i] == '}' )
				path[i] = '-';
		FILE* fp = fopen(path, "w");
		if ( !fp )
		{
			fprintf(stderr, "%s: %m\n", path);
			exit(1);
		}
		if ( declaration->optional )
			fprintf(fp, "/*optional*/\n");
		if ( declaration->options && strcmp(declaration->options, "CX") != 0 )
		{
			fprintf(fp, "/*[%s]*/\n", declaration->options);
			if ( strstr(declaration->options, "XSI") )
			{
				fprintf(fp, "#if 202405L <= _POSIX_C_SOURCE\n");
				fprintf(fp, "#define _XOPEN_SOURCE 800\n");
				fprintf(fp, "#elif 200809L <= _POSIX_C_SOURCE\n");
				fprintf(fp, "#define _XOPEN_SOURCE 700\n");
				fprintf(fp, "#endif\n");
			}
		}
		fprintf(fp, "#include <%s>\n", header);
		// TODO: Test generic functions in a better manner. Although it's
		//       unclear how it is even possible to do it precisely.
		if ( declaration->type_mask == REQUIRED_TYPE(TYPE_DEFINITION) ||
		     declaration->type_mask == REQUIRED_TYPE(TYPE_GENERIC) )
		{
			fprintf(fp, "#ifndef %s\n#error \"%s is not defined\"\n#endif\n",
			        declaration->name, declaration->name);
		}
		else if ( declaration->type_mask == REQUIRED_TYPE(TYPE_TYPE) )
		{
			fprintf(fp, "%s* foo;\n", declaration->name);
		}
		else if ( declaration->type_mask == REQUIRED_TYPE(TYPE_STRUCTURE) )
		{
			const char* ptr = declaration->incomplete ? "*" : "";
			fprintf(fp, "struct %s%s foo;\n", declaration->name, ptr);
		}
		else if ( declaration->type_mask == REQUIRED_TYPE(TYPE_UNION) )
		{
			const char* ptr = declaration->incomplete ? "*" : "";
			fprintf(fp, "union %s%s foo;\n", declaration->name, ptr);
		}
		else if ( declaration->type_mask == REQUIRED_TYPE(TYPE_ENUMERATION) )
		{
			const char* ptr = declaration->incomplete ? "*" : "";
			fprintf(fp, "enum %s%s foo;\n", declaration->name, ptr);
		}
		else if ( declaration->type_mask == REQUIRED_TYPE(TYPE_FUNCTION) ||
		          declaration->type_mask == (REQUIRED_TYPE(TYPE_FUNCTION) |
		                                     OPTIONAL_TYPE(TYPE_DEFINITION)) ||
		          declaration->type_mask == (OPTIONAL_TYPE(TYPE_FUNCTION) |
		                                     OPTIONAL_TYPE(TYPE_DEFINITION)) )
		{
			if ( declaration->type_mask == (OPTIONAL_TYPE(TYPE_FUNCTION) |
		                                     OPTIONAL_TYPE(TYPE_DEFINITION)) )
				fprintf(fp, "#ifndef %s\n", declaration->name);
			else if ( declaration->type_mask & OPTIONAL_TYPE(TYPE_DEFINITION) )
				fprintf(fp, "#ifdef %s\n#undef %s\n#endif\n", declaration->name,
				        declaration->name);
			const char* sig = declaration->sig;
			if ( !strncmp(sig, "_Noreturn", strlen("_Noreturn")) )
				sig += strlen("_Noreturn");
			size_t name_at = 0;
			while ( sig[name_at] )
			{
				if ( !strncmp(sig + name_at, declaration->name,
				              strlen(declaration->name)) &&
				     (!sig[name_at + strlen(declaration->name)] ||
				      sig[name_at + strlen(declaration->name)] == '(' ||
				      sig[name_at + strlen(declaration->name)] == ')' ||
				      sig[name_at + strlen(declaration->name)] == '[' ||
				      (sig[name_at + strlen(declaration->name)+0] == ' ' &&
				       sig[name_at + strlen(declaration->name)+1] == '(')) )
					break;
				name_at++;
			}
			fwrite(sig, 1, name_at, fp);
			fprintf(fp, "(*foo)");
			size_t name_end = name_at + strlen(declaration->name);
			if ( sig[name_end] == '[' )
			{
				name_end++;
				while ( sig[name_end] && sig[name_end] != ']' )
					name_end++;
				if ( sig[name_end] == ']' )
					name_end++;
			}
			fputs(sig + name_end, fp);
			fprintf(fp, " = %s;\n", declaration->name);
			if ( declaration->type_mask == (OPTIONAL_TYPE(TYPE_FUNCTION) |
		                                     OPTIONAL_TYPE(TYPE_DEFINITION)) )
				fprintf(fp, "#endif\n");
		}
		else if ( declaration->type_mask == REQUIRED_TYPE(TYPE_STRUCTURE_MEMBER) ||
		          declaration->type_mask == REQUIRED_TYPE(TYPE_UNION_MEMBER) )
		{
			fprintf(fp, "void foo(%s* bar)\n", declaration->parent);
			fprintf(fp, "{\n");
			fprintf(fp, "\t");
			const char* sig = declaration->sig;
			size_t name_at = 0;
			while ( sig[name_at] )
			{
				if ( !strncmp(sig + name_at, declaration->name,
				              strlen(declaration->name)) &&
				     (!sig[name_at + strlen(declaration->name)] ||
				      sig[name_at + strlen(declaration->name)] == '(' ||
				      sig[name_at + strlen(declaration->name)] == ')' ||
				      sig[name_at + strlen(declaration->name)] == '[' ||
				      (sig[name_at + strlen(declaration->name)+0] == ' ' &&
				       sig[name_at + strlen(declaration->name)+1] == '(')) )
					break;
				name_at++;
			}
			fwrite(sig, 1, name_at, fp);
			fputc('*', fp);
			fprintf(fp, "qux");
			size_t name_end = name_at + strlen(declaration->name);
			if ( sig[name_end] == '[' )
			{
				name_end++;
				while ( sig[name_end] && sig[name_end] != ']' )
					name_end++;
				if ( sig[name_end] == ']' )
					name_end++;
			}
			fputs(sig + name_end, fp);
			fprintf(fp, " = ");
			if ( sig[name_at + strlen(declaration->name)] != '[' )
				fputc('&', fp);
			fprintf(fp, "bar->%s;\n", declaration->name);
			fprintf(fp, "\t(void) qux;\n");
			fprintf(fp, "}\n");
		}
		else if ( declaration->type_mask == REQUIRED_TYPE(TYPE_ENUMERATION_MEMBER) )
		{
			const char* parent_type =
				declaration->parent ? declaration->parent : "int";
			fprintf(fp, "%s foo = %s;\n", parent_type, declaration->name);
		}
		else if ( declaration->type_mask == REQUIRED_TYPE(TYPE_EXTERNAL) ||
		          declaration->type_mask == REQUIRED_TYPE(TYPE_SYMBOLIC_CONSTANT) )
		{
			const char* sig = declaration->sig;
			size_t name_at = 0;
			while ( sig[name_at] )
			{
				if ( !strncmp(sig + name_at, declaration->name,
				              strlen(declaration->name)) &&
				     (!sig[name_at + strlen(declaration->name)] ||
				      sig[name_at + strlen(declaration->name)] == '(' ||
				      sig[name_at + strlen(declaration->name)] == ')' ||
				      sig[name_at + strlen(declaration->name)] == '[' ||
				      (sig[name_at + strlen(declaration->name)+0] == ' ' &&
				       sig[name_at + strlen(declaration->name)+1] == '(')) )
					break;
				name_at++;
			}
			if ( !name_at )
				fputs("int", fp);
			else
				fwrite(sig, 1, name_at, fp);
			if ( declaration->type_mask == REQUIRED_TYPE(TYPE_SYMBOLIC_CONSTANT) )
				fputs(" const ", fp);
			if ( declaration->type_mask == REQUIRED_TYPE(TYPE_EXTERNAL) )
				fputc('*', fp);
			fprintf(fp, "foo");
			size_t name_end = name_at + strlen(declaration->name);
			if ( sig[name_end] == '[' )
			{
				name_end++;
				while ( sig[name_end] && sig[name_end] != ']' )
					name_end++;
				if ( sig[name_end] == ']' )
					name_end++;
			}
			fputs(sig + name_end, fp);
			fprintf(fp, " = ");
			if ( declaration->type_mask == REQUIRED_TYPE(TYPE_EXTERNAL) &&
			     sig[name_at + strlen(declaration->name)] != '[' )
				fputc('&', fp);
			fprintf(fp, "%s;\n", declaration->name);
		}
		else if ( declaration->type_mask == REQUIRED_TYPE(TYPE_EXPRESSION) )
		{
			fprintf(fp, "void foo(void)\n");
			fprintf(fp, "{\n");
			fprintf(fp, "\t");
			const char* sig = declaration->sig;
			size_t name_at = 0;
			while ( sig[name_at] )
			{
				if ( !strncmp(sig + name_at, declaration->name,
				              strlen(declaration->name)) &&
				     (!sig[name_at + strlen(declaration->name)] ||
				      sig[name_at + strlen(declaration->name)] == '(' ||
				      sig[name_at + strlen(declaration->name)] == ')' ||
				      sig[name_at + strlen(declaration->name)] == '[' ||
				      (sig[name_at + strlen(declaration->name)+0] == ' ' &&
				       sig[name_at + strlen(declaration->name)+1] == '(')) )
					break;
				name_at++;
			}
			fwrite(sig, 1, name_at, fp);
			fprintf(fp, "bar");
			fprintf(fp, " = ");
			fprintf(fp, "%s;\n", declaration->name);
			fprintf(fp, "\t(void) bar;\n");
			fprintf(fp, "}\n");
		}
		else
		{
			fprintf(stderr, "%s: Don't know how to test this\n", path);
		}
		fprintf(fp, "int main(void) { return 0; }\n");
		fclose(fp);
		free(path);
	}
}

void parse_path(const char* path)
{
	FILE* fp = fopen(path, "r");
	if ( !fp )
		err(1, "%s", path);
	char* input = NULL;
	size_t input_size = 0;
	if ( getdelim(&input, &input_size, 0, fp) < 0 )
		err(1, "getdelim: %s", path);
	bool in_pre = false;
	size_t o = 0;
	size_t input_length = strlen(input);
	size_t paren = 0;
	size_t tag = 0;
	bool had_newline = false;
	for ( size_t i = 0; i < input_length; i++ )
	{
		if ( !strncmp(input + i, "<pre>", 5) )
			in_pre = true, had_newline = false, paren = 0, tag = 0;
		else if ( !strncmp(input + i, "</pre>", 6) )
			in_pre = false;
		char c = input[i];
		if ( in_pre && c == '(' )
			paren++;
		else if ( in_pre && paren && c == ')' )
			paren--;
		if ( in_pre && c == '<' )
			tag++;
		else if ( in_pre && tag && c == '>' )
			tag--;
		if ( (!in_pre || paren || tag) && c == '\n' )
		{
			had_newline = true;
			c = ' ';
		}
		else if ( c == '\n' )
			had_newline = false;
		if ( had_newline && o && input[o-1] == ' ' && c == ' ' )
			continue;
		input[o++] = c;
	}
	input[o] = '\0';
	parse(input, path);
	free(input);
	fclose(fp);
}

int main(int argc, char* argv[])
{
	for ( int i = 1; i < argc; i++ )
		parse_path(argv[i]);
	generate();
	return 0;
}
