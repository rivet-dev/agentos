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
 * html.c
 * Generate report with os-test results.
 */

// Use POSIX 2008 instead of 2024 for greater compatibility.
#define _POSIX_C_SOURCE 200809L

#include <sys/stat.h>
#include <sys/wait.h>

#include <dirent.h>
#include <errno.h>
#include <libgen.h>
#include <locale.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "errors.h"

enum outcome
{
	GOOD,
	BAD,
	UNKNOWN,
	UNRATED,
	MISSING_OPTIONAL,
	OUTSIDE_LIBC,
	EXTENSION,
	PREVIOUS_POSIX,
	COMPILE_ERROR,
	INCOMPATIBLE,
	MISSING_HEADER,
	POLLUTION,
	UNDECLARED,
	UNDEFINED,
	UNKNOWN_TYPE,
	NONE,
	OUTCOME_MAX,
};

static const char* outcome_names[OUTCOME_MAX] =
{
	[GOOD] = "good",
	[BAD] = "bad",
	[UNKNOWN] = "unknown",
	[UNRATED] = "unrated",
	[MISSING_OPTIONAL] = "missing_optional",
	[OUTSIDE_LIBC] = "outside_libc",
	[EXTENSION] = "extension",
	[PREVIOUS_POSIX] = "previous_posix",
	[COMPILE_ERROR] = "compile_error",
	[INCOMPATIBLE] = "incompatible",
	[MISSING_HEADER] = "missing_header",
	[POLLUTION] = "pollution",
	[UNDECLARED] = "undeclared",
	[UNDEFINED] = "undefined",
	[UNKNOWN_TYPE] = "unknown_type",
	[NONE] = "none",
};

enum judgement
{
	JUDGEMENT_GOOD,
	JUDGEMENT_PARTIAL,
	JUDGEMENT_BAD,
	JUDGEMENT_NONE,
	JUDGEMENT_MAX,
};

static const char* judgement_names[JUDGEMENT_MAX] =
{
	[JUDGEMENT_GOOD] = "good",
	[JUDGEMENT_PARTIAL] = "partial",
	[JUDGEMENT_BAD] = "bad",
	[JUDGEMENT_NONE] = "none",
};

static const char* option_names[] =
{
	"ADV\0Advisory Information",
	"CD\0C-Language Development Utilities",
	"CPT\0Process CPU-Time Clocks",
	"CX\0Extension to the ISO C standard",
	"DC\0Device Control",
	"FR\0FORTRAN Runtime Utilities",
	"FSC\0File Synchronization",
	"IP6\0IPV6",
	"MC1\0Non-Robust Mutex Priority Protection or Non-Robust Mutex Priority Inheritance or Robust Mutex Priority Protection or Robust Mutex Priority Inheritance",
	"ML\0Process Memory Locking",
	"MLR\0Range Memory Locking",
	"MSG\0Message Passing",
	"MX\0IEC 60559 Floating-Point",
	"MXC\0IEC 60559 Complex Floating-Point",
	"MXX\0IEC 60559 Floating-Point Extension",
	"OB\0Obsolescent",
	"OF\0Output Format Incompletely Specified",
	"PIO\0Prioritized Input and Output",
	"PS\0Process Scheduling",
	"RPI\0Robust Mutex Priority Inheritance",
	"RPP\0Robust Mutex Priority Protection",
	"RS\0Raw Sockets",
	"SD\0Software Development Utilities",
	"SHM\0Shared Memory Objects",
	"SIO\0Synchronized Input and Output",
	"SPN\0Spawn",
	"SS\0Process Sporadic Server",
	"TCT\0Thread CPU-Time Clocks",
	"TPI\0Non-Robust Mutex Priority Inheritance",
	"TPP\0Non-Robust Mutex Priority Protection",
	"TPS\0Thread Execution Scheduling",
	"TSA\0Thread Stack Address Attribute",
	"TSH\0Thread Process-Shared Synchronization",
	"TSP\0Thread Sporadic Server",
	"TSS\0Thread Stack Size Attribute",
	"TYM\0Typed Memory Objects",
	"UP\0User Portability Utilities",
	"UU\0UUCP Utilities",
	"XSI\0X/Open System Interfaces",
};

struct statistics
{
	size_t counters[OUTCOME_MAX];
};

static bool shorten_results;
static char* expectations_directory;
static char* html_footer;
static char* html_header;
static char* html_index;
static char* html_introduction;
static char* html_legend;
static char* html_legend_include;
static char* html_legend_namespace;
static char* html_legend_overview;
static bool json_had_output = false;
static bool json_lines = false;
static char* os_directory;
static char* output_directory;
static char* output_file;
static char* suites_directory;

static char** oss;
static size_t oss_count;
static size_t oss_length;

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

// Sort until the first period per strcmp, then the rest if equal.
int no_extension_sort(const struct dirent** a, const struct dirent** b)
{
	const char* a_name = (const char*) ((*a)->d_name);
	const char* b_name = (const char*) ((*b)->d_name);
	char* a_str = strndup(a_name, strcspn(a_name, "."));
	char* b_str = strndup(b_name, strcspn(b_name, "."));
	if ( !a_str || !b_str )
		err(1, "malloc");
	int result = strcmp(a_str, b_str);
	free(a_str);
	free(b_str);
	if ( !result )
		result = strcmp(a_name, b_name);
	return result;
}

// Sort null first then by strcmp.
static int strcmp_null(const char* a, const char* b)
{
	if ( !a && !b )
		return 0;
	if ( !a && b )
		return -1;
	if ( a && !b )
		return 1;
	return strcmp(a, b);
}

static int strcmp_null_indirect(const void* a_ptr, const void* b_ptr)
{
	const char* a = *(const char* const*) a_ptr;
	const char* b = *(const char* const*) b_ptr;
	return strcmp_null(a, b);
}

static enum outcome outcome_parse(const char* string)
{
	for ( enum outcome outcome = 0; outcome < OUTCOME_MAX; outcome++ )
	{
		if ( !strcmp(string, outcome_names[outcome]) )
			return outcome;
	}
	return OUTCOME_MAX;
}

static enum judgement judge(enum outcome outcome)
{
	switch ( outcome )
	{
	case GOOD:
	case MISSING_OPTIONAL:
	case OUTSIDE_LIBC:
		return JUDGEMENT_GOOD;
	case EXTENSION:
	case PREVIOUS_POSIX:
		return JUDGEMENT_PARTIAL;
	case UNRATED:
	case UNKNOWN:
	case NONE:
		return JUDGEMENT_NONE;
	default:
		return JUDGEMENT_BAD;
	}
}

static void json_output_string(FILE* output, const char* string)
{
	if ( !string )
	{
		fputs("null", output);
		return;
	}
	fputc('"', output);
	for ( size_t i = 0; string[i]; i++ )
	{
		if ( string[i] == '"' )
			fputs("\\\"", output);
		else if ( string[i] == '\\' )
			fputs("\\\\", output);
		else if ( string[i] == '\n' )
			fputs("\\n", output);
		else
			fputc((unsigned char) string[i], output);
	}
	fputc('"', output);
}

static void json_output_header(FILE* output)
{
	(void) output;
}

static void json_output_footer(FILE* output)
{
	(void) output;
}

static void json_output_title(FILE* output, const char* suite)
{
	(void) output;
	(void) suite;
}

static void json_output_introduction(FILE* output, const char* suite)
{
	(void) output;
	(void) suite;
}

static void json_output_legend(FILE* output, const char* suite)
{
	(void) output;
	(void) suite;
}

static void json_output_legend_include(FILE* output, const char* suite)
{
	(void) output;
	(void) suite;
}

static void json_output_legend_namespace(FILE* output, const char* suite)
{
	(void) output;
	(void) suite;
}

static void json_output_legend_overview(FILE* output, const char* suite)
{
	(void) output;
	(void) suite;
}

static void json_output_suites(FILE* output, const char* suite,
                               const char* const* subsuites,
                               size_t subsuites_count)
{
	(void) output;
	(void) suite;
	(void) subsuites;
	(void) subsuites_count;
}

static void json_output_section(FILE* output, const char* suite)
{
	(void) output;
	(void) suite;
}

static void json_output_table_begin(FILE* output, const char* suite)
{
	(void) output;
	(void) suite;
}

static void json_output_table_begin_options(FILE* output, const char* suite,
                                            const char* options)
{
	(void) output;
	(void) suite;
	(void) options;
}

static void json_output_result(FILE* output, const char* suite,
                               const char* test, bool optional,
                               const char* options, const char* os,
                               const char* result, enum outcome outcome)
{
	if ( !json_lines && json_had_output )
		fputs(",\n", output);
	fprintf(output, "{\"suite\": ");
	json_output_string(output, suite);
	fprintf(output, ", \"test\": ");
	json_output_string(output, test);
	fprintf(output, ", \"optional\": %s", optional ? "true" : "false");
	fprintf(output, ", \"options\": ");
	json_output_string(output, options);
	fprintf(output, ", \"os\": ");
	json_output_string(output, os);
	fprintf(output, ", \"result\": ");
	json_output_string(output, result);
	fprintf(output, ", \"outcome\": ");
	json_output_string(output, outcome_names[outcome]);
	fprintf(output, ", \"judgement\": ");
	json_output_string(output, judgement_names[judge(outcome)]);
	fprintf(output, "}");
	if ( json_lines )
		fputc('\n', output);
	json_had_output = true;
}

static void json_output_results(FILE* output, const char* suite,
                                const char* test, bool optional,
                                const char* options, char** result_for_os,
                                enum outcome* outcome_for_os,
                                char** expectations,
                                size_t expectations_count)
{
	(void) expectations;
	(void) expectations_count;
	for ( size_t o = 0; o < oss_count; o++ )
		json_output_result(output, suite, test, optional, options, oss[o],
		                   result_for_os[o], outcome_for_os[o]);
}

static void json_output_table_statistics(FILE* output, const char* suite,
                                         const char* subsuite,
                                         const char* options,
                                         struct statistics* statistics_for_os)
{
	(void) output;
	(void) suite;
	(void) subsuite;
	(void) options;
	(void) statistics_for_os;
}

static void json_output_table_end_options(FILE* output, const char* suite,
                                          const char* options)
{
	(void) output;
	(void) suite;
	(void) options;
}

static void json_output_table_end(FILE* output, const char* suite)
{
	(void) output;
	(void) suite;
}

static FILE* json_output_begin_suite(FILE* global_output, const char* suite)
{
	(void) suite;
	return global_output;
}

static void json_output_end_suite(FILE* global_output, FILE* output,
                                  const char* suite)
{
	(void) suite;
	if ( ferror(output) || fflush(output) == EOF )
		err(1, "writing output");
	if ( global_output != output && output != stdout )
		fclose(output);
}

static FILE* json_output_begin(void)
{
	FILE* global_output = output_file ? fopen(output_file, "w") : stdout;
	if ( !global_output )
		err(1, "%s", output_file);
	if ( !json_lines )
		fputs("[\n", global_output);
	return global_output;
}

static void json_output_end(FILE* global_output)
{
	if ( !global_output )
		return;
	if ( !json_lines )
	{
		if ( json_had_output )
			fputc('\n', global_output);
		fputs("]\n", global_output);
	}
	if ( ferror(global_output) || fflush(global_output) == EOF )
		err(1, "writing output");
	if ( global_output != stdout )
		fclose(global_output);
}

static void html_output_header(FILE* output)
{
	char* data = read_text_file(html_header);
	if ( !data )
		err(1, "%s", html_header);
	fputs(data, output);
	free(data);
}

static void html_output_footer(FILE* output)
{
	char* data = read_text_file(html_footer);
	if ( !data )
		err(1, "%s", html_footer);
	fputs(data, output);
	free(data);
}

static void html_output_escape(FILE* output, const char* string)
{
	// TODO: Escape HTML.
	fputs(string, output);
}

static void html_output_escape_fragment(FILE* output, const char* string)
{
	// TODO: Escape HTML.
	for ( size_t i = 0; string[i]; i++ )
	{
		if ( string[i] == ' ' )
			fputs("_and_", output);
		else if ( string[i] == '|' )
			fputs("_or_", output);
		else
			fputc((unsigned char) string[i], output);
	}
}

// Output the page title and link to parent pages.
static void html_output_title(FILE* output, const char* suite)
{
	fprintf(output, "      <h1>");
	if ( !suite || !output_directory )
		html_output_escape(output, "os-test");
	else
	{
		size_t depth = 1;
		for ( size_t i = 0; suite[i]; i++ )
			if ( suite[i] == '/' )
				depth++;
		html_output_escape(output, "<a href=\"");
		for ( size_t d = 0; d < depth; d++ )
			html_output_escape(output, "../");
		html_output_escape(output, html_index);
		html_output_escape(output, "\">os-test</a>");
		size_t i = 0;
		while ( suite[i] )
		{
			depth--;
			html_output_escape(output, " &gt; ");
			size_t length = strcspn(suite + i, "/");
			char* name = strndup(suite + i, length);
			if ( !name )
				err(1, "malloc");
			if ( depth )
			{
				html_output_escape(output, "<a href=\"");
				for ( size_t d = 0; d < depth; d++ )
					html_output_escape(output, "../");
				html_output_escape(output, html_index);
				html_output_escape(output, "\">");
			}
			html_output_escape(output, name);
			if ( depth )
				html_output_escape(output, "</a>");
			free(name);
			i += length;
			if ( suite[i] == '/' )
				i++;
		}
	}
	fprintf(output, "</h1>\n");
}

static void html_output_introduction(FILE* output, const char* suite)
{
	if ( !suite && html_introduction )
	{
		char* data = read_text_file(html_introduction);
		if ( !data )
			err(1, "%s", html_introduction);
		fputs(data, output);
		free(data);
	}
}

static void html_output_legend(FILE* output, const char* suite)
{
	(void) suite;
	char* data = read_text_file(html_legend);
	if ( !data )
		err(1, "%s", html_legend);
	fputs(data, output);
	free(data);
}

static void html_output_legend_include(FILE* output, const char* suite)
{
	(void) suite;
	char* data = read_text_file(html_legend_include);
	if ( !data )
		err(1, "%s", html_legend_include);
	fputs(data, output);
	free(data);
}

static void html_output_legend_namespace(FILE* output, const char* suite)
{
	(void) suite;
	char* data = read_text_file(html_legend_namespace);
	if ( !data )
		err(1, "%s", html_legend_namespace);
	fputs(data, output);
	free(data);
}

static void html_output_legend_overview(FILE* output, const char* suite)
{
	(void) suite;
	char* data = read_text_file(html_legend_overview);
	if ( !data )
		err(1, "%s", html_legend_overview);
	fputs(data, output);
	free(data);
}

// Output list of suites inside the current suite.
static void html_output_suites(FILE* output, const char* suite,
                               const char* const* subsuites,
                               size_t subsuites_count)
{
	const char* title = suite ? "Subsuites" : "Suites";
	const char* id = suite ? "subsuites" : "suites";
	fprintf(output, "      <h2 id=\"");
	html_output_escape_fragment(output, id);
	fprintf(output, "\">");
	fprintf(output, "<a href=\"#");
	html_output_escape_fragment(output, id);
	fprintf(output, "\">");
	html_output_escape(output, title);
	fprintf(output, "</a>");
	fprintf(output, "</h2>\n");
	fprintf(output, "      <p>");
	if ( suite )
	{
		fprintf(output, "The ");
		html_output_escape(output, suite);
		fprintf(output, " suite contains the following subsuite");
	}
	else
		fprintf(output, "os-test contains the following suite");
	if ( 2 <= subsuites_count )
		fprintf(output, "s");
	fprintf(output, ":</p>\n");
	fprintf(output, "      <ul>\n");
	for ( size_t i = 0; i < subsuites_count; i++ )
	{
		const char* subsuite = subsuites[i];
		const char* subsuite_basename = strrchr(subsuite, '/');
		char* subsuite_directory = join_paths(suites_directory, subsuite);
		if ( !subsuite_directory )
			err(1, "malloc");
		char* subsuite_readme = join_paths(subsuite_directory, "README");
		if ( !subsuite_readme )
			err(1, "malloc");
		char* readme = read_text_file(subsuite_readme);
		if ( !readme && errno != ENOENT )
			err(1, "%s", subsuite_readme);
		free(subsuite_readme);
		free(subsuite_directory);
		subsuite_basename =
			subsuite_basename ? subsuite_basename + 1 : subsuite;
		fprintf(output, "      <li>");
		fprintf(output, "<a href=\"");
		html_output_escape(output, subsuite_basename);
		fprintf(output, "/");
		if ( html_index )
			html_output_escape(output, html_index);
		fprintf(output, "#");
		html_output_escape_fragment(output, subsuite_basename);
		fprintf(output, "\">");
		html_output_escape(output, subsuite_basename);
		fprintf(output, "</a>");
		if ( readme )
		{
			fprintf(output, " - ");
			readme[strcspn(readme, "\n")] = '\0';
			html_output_escape(output, readme);
		}
		fprintf(output, "</li>\n");
		free(readme);
	}
	fprintf(output, "      </ul>\n");
}

// Output section for this suite.
static void html_output_section(FILE* output, const char* suite)
{
	const char* title = suite ? suite : "Results";
	const char* id = suite ? suite : "results";
	const char* title_basename = strrchr(title, '/');
	const char* id_basename = strrchr(id, '/');
	title_basename = title_basename ? title_basename + 1 : title;
	id_basename = id_basename ? id_basename + 1 : id;
	fprintf(output, "      <h2 id=\"");
	html_output_escape_fragment(output, id_basename);
	fprintf(output, "\">");
	fprintf(output, "<a href=\"#");
	html_output_escape_fragment(output, id_basename);
	fprintf(output, "\">");
	html_output_escape(output, title_basename);
	fprintf(output, "</a>");
	fprintf(output, "</h2>\n");
	if ( !suite )
		return;
	// Include the README for the suite if markdown(1) is installed.
	char* suite_directory = join_paths(suites_directory, suite);
	if ( !suite_directory )
		err(1, "malloc");
	char* suite_readme = join_paths(suite_directory, "README");
	if ( !suite_readme )
		err(1, "malloc");
	if ( !access(suite_readme, F_OK) )
	{
		fflush(output);
		pid_t child = fork();
		if ( child < 0 )
			err(1, "fork");
		if ( !child )
		{
			if ( dup2(fileno(output), 1) != 1 )
			{
				warn("dup2");
				_exit(1);
			}
			execlp("markdown", "markdown", suite_readme, (char*) NULL);
			if ( errno != ENOENT )
				warn("%s", "markdown");
			_exit(127);
		}
		int status;
		if ( waitpid(child, &status, 0) < 0 )
			err(1, "waitpid");
		if ( !WIFEXITED(status) ||
		     (WEXITSTATUS(status) != 0 && WEXITSTATUS(status) != 127) )
			exit(1);
	}
	else if ( errno != ENOENT )
		err(1, "%s", suite_readme);
	free(suite_readme);
	free(suite_directory);
}

static void html_output_table_begin(FILE* output, const char* suite)
{
	(void) suite;
	fprintf(output, "      <table class=\"big-comparison\">\n");
	fprintf(output, "        <thead>\n");
	fprintf(output, "          <tr>\n");
	fprintf(output, "            <th></th>\n");
	for ( size_t o = 0; o < oss_count; o++ )
	{
		const char* os = oss[o];
		char* os_dir = join_paths(os_directory, os);
		if ( !os_dir )
			err(1, "malloc");
		char* uname_path = join_paths(os_dir, "uname.out");
		if ( !uname_path )
			err(1, "malloc");
		char* uname = read_text_file(uname_path);
		if ( !uname && errno != ENOENT )
			err(1, "%s", uname_path);
		size_t uname_length = uname ? strlen(uname) : 0;
		if ( uname_length && uname[uname_length-1] == '\n' )
			uname[uname_length-1] = '\0';
		fprintf(output, "            <th>");
		html_output_escape(output, os);
		if ( uname )
		{
			fprintf(output, "<br>");
			html_output_escape(output, uname);
		}
		fprintf(output, "</th>\n");
		free(uname);
		free(uname_path);
		free(os_dir);
	}
	fprintf(output, "          </tr>\n");
	fprintf(output, "        </thead>\n");
	fprintf(output, "        <tbody>\n");
}

static void html_output_table_begin_options(FILE* output, const char* suite,
                                            const char* options)
{
	(void) suite;
	if ( options )
	{
		const char* title = options;
		const char* title_basename = strrchr(title, '/');
		title_basename = title_basename ? title_basename + 1 : title;
		fprintf(output, "          <tr>\n");
		fprintf(output, "            <th colspan=\"%zu\">\n", 1 + oss_count);
		fprintf(output, "              <span class=\"option\" id=\"");
		html_output_escape_fragment(output, title_basename);
		fprintf(output, "\">");
		fprintf(output, "<a href=\"#");
		html_output_escape_fragment(output, title_basename);
		fprintf(output, "\">");
		html_output_escape(output, "Optional: ");
		html_output_escape(output, title_basename);
		fprintf(output, "</a>");
		fprintf(output, "</span><br>\n");
		size_t i = 0;
		while ( options[i] )
		{
			size_t length = strcspn(options + i, " |");
			for ( size_t n = 0;
			      n < sizeof(option_names) / sizeof(option_names[0]);
			      n++ )
			{
				const char* name = option_names[n];
				size_t namelen = strlen(name);
				if ( !strncmp(options + i, name, namelen) &&
				     (!options[i + namelen] ||
				      options[i + namelen] == ' ' ||
				      options[i + namelen] == '|') )
				{
					html_output_escape(output, name + namelen + 1);
					break;
				}
			}
			i += length;
			if ( options[i] == ' ' )
				i++, html_output_escape(output, "&ensp;<i>and</i>&ensp;");
			else if ( options[i] == '|' )
				i++, html_output_escape(output, "&ensp;<i>or</i>&ensp;");
			else if ( options[i] )
				i++;
		}
		fprintf(output, "            </th>\n");
		fprintf(output, "          </tr>\n");
	}
}

static void html_output_results(FILE* output, const char* suite,
                                const char* test, bool optional,
                                const char* options, char** result_for_os,
                                enum outcome* outcome_for_os,
                                char** expectations,
                                size_t expectations_count)
{
	(void) suite;
	(void) optional;
	(void) options;

	char* suite_dir = join_paths(suites_directory, suite);
	if ( !suite_dir )
		err(1, "malloc");
	char* output_dir =
		output_directory ? join_paths(output_directory, suite) : NULL;
	if ( output_directory && !output_dir )
		err(1, "malloc");
	if ( output_directory && mkdir_p(output_dir, 0777) < 0 )
		err(1, "%s", output_dir);

	fprintf(output, "          <tr>\n");

	fprintf(output, "            <th id=\"");
	html_output_escape_fragment(output, test);
	fprintf(output, "\">");
	fprintf(output, "<a href=\"#");
	html_output_escape_fragment(output, test);
	fprintf(output, "\">§</a> <a href=\"");
	html_output_escape(output, test);
	fprintf(output, ".c\">");
	html_output_escape(output, test);
	fprintf(output, "</a></th>\n");

	const char* variants[OUTCOME_MAX][5] = {0};
	size_t num_variants[OUTCOME_MAX] = {0};
	for ( size_t o = 0; o < oss_count; o++ )
	{
		const char* result = result_for_os[o];
		enum outcome outcome = outcome_for_os[o];
		if ( outcome == GOOD || outcome == BAD ||
		     outcome == UNKNOWN || outcome == UNRATED )
		{
			for ( size_t v = 0; v < 5; v++ )
			{
				if ( outcome == GOOD && v < expectations_count &&
				     strcmp(expectations[v], result) != 0 )
					continue;
				if ( !variants[outcome][v] )
				{
					variants[outcome][v] = result;
					num_variants[outcome]++;
					break;
				}
				if ( !strcmp(variants[outcome][v], result) )
					break;
			}
		}
	}

	for ( size_t o = 0; o < oss_count; o++ )
	{
		const char* os = oss[o];

		// See if the test result has an .err error message file.
		char* result_os_dir = join_paths(os_directory, os);
		if ( !result_os_dir )
			err(1, "malloc");
		char* result_suite_dir = join_paths(result_os_dir, suite);
		if ( !result_suite_dir )
			err(1, "malloc");
		char* err_name;
		if ( format_string(&err_name, "%s.err", test) < 0 )
			err(1, "malloc");
		char* err_path = join_paths(result_suite_dir, err_name);
		if ( !err_path )
			err(1, "malloc");
		char* err_msg = read_text_file(err_path);
		if ( !err_msg && errno != ENOENT )
			err(1, "%s", err_path);
		free(err_path);
		free(err_name);
		free(result_suite_dir);
		free(result_os_dir);

		// If so, copy the .err error message to the output directory.
		bool has_err = err_msg;
		if ( output_dir && has_err )
		{
			char* err_output_path;
			if ( format_string(&err_output_path, "%s/%s.%s.err",
			                   output_dir, test, os) < 0 )
				err(1, "malloc");
			FILE* err_output = fopen(err_output_path, "w");
			if ( !err_output )
				err(1, "%s", err_output_path);
			fputs(err_msg, err_output);
			if ( ferror(err_output) || fflush(err_output) == EOF )
				err(1, "write: %s", err_output_path);
			fclose(err_output);
			free(err_output_path);
			free(err_msg);
		}

		// Count the number of result variants for the same outcome, so they can
		// be colored differently if the results are not unanimous.
		const char* result = result_for_os[o];
		enum outcome outcome = outcome_for_os[o];
		const char* css_class = outcome_names[outcome];
		size_t variant = 0;
		if ( (outcome == GOOD || outcome == BAD ||
		      outcome == UNKNOWN || outcome == UNRATED) &&
		     2 <= num_variants[outcome] )
		{
			for ( size_t v = 0; v < 5; v++ )
			{
				if ( variants[outcome][v] &&
				     !strcmp(variants[outcome][v], result) )
				{
					variant = v + 1;
					break;
				}
			}
		}

		fprintf(output, "            <td class=\"");
		html_output_escape(output, css_class);
		if ( 0 < variant )
			fprintf(output, "-%zu", variant);
		fprintf(output, "\">");
		html_output_escape(output, os);
		fprintf(output, ": ");
		if ( has_err )
		{
			fprintf(output, "<a href=\"");
			html_output_escape(output, test);
			fprintf(output, ".");
			html_output_escape(output, os);
			fprintf(output, ".err");
			fprintf(output, "\">");
		}
		html_output_escape(output, outcome_names[outcome]);
		if ( has_err )
			fprintf(output, "</a>");
		if ( result )
		{
			// These results otherwise make the udp suite html page too large.
			if ( shorten_results &&
			     strstr(result, "inet_aton might be broken") )
				result = "RELIBC PANIC: inet_aton might be broken";
			if ( shorten_results &&
			     strstr(result, "setsockopt") &&
			     strstr(result, "- unknown option") )
				result = "setsockopt: unknown option";
			fprintf(output, "<br>");
			size_t i = 0;
			while ( result[i] )
			{
				if ( result[i] == '\n' )
				{
					if ( !result[i+1] )
						break;
					fprintf(output, "<br>");
					i++;
					continue;
				}
				size_t length = strcspn(result + i, "\n");
				char* substring = strndup(result + i, length);
				if ( !substring )
					err(1, "malloc");
				html_output_escape(output, substring);
				free(substring);
				i += length;
			}
		}
		fprintf(output, "</td>\n");
	}

	fprintf(output, "          </tr>\n");

	free(suite_dir);
	free(output_dir);
}

static void html_output_table_statistics(FILE* output, const char* suite,
                                         const char* subsuite,
                                         const char* options,
                                         struct statistics* statistics_for_os)
{
	(void) suite;

	const char* name = subsuite ? subsuite : options ? options : "overall";
	const char* name_basename = strrchr(name, '/');
	name_basename = name_basename ? name_basename + 1 : name;

	fprintf(output, "          <tr class=\"percentage\">\n");

	fprintf(output, "            <th");
	if ( subsuite || !options )
	{
		fprintf(output, " id=\"");
		if ( options && subsuite )
		{
			html_output_escape_fragment(output, options);
			fprintf(output, "-");
		}
		html_output_escape_fragment(output, name_basename);
		fprintf(output, "\"");
	}
	fprintf(output, ">");
	fprintf(output, "<a href=\"#");
	if ( options && subsuite )
	{
		html_output_escape_fragment(output, options);
		fprintf(output, "-");
	}
	html_output_escape_fragment(output, name_basename);
	fprintf(output, "\">§</a> ");
	if ( subsuite )
	{
		fprintf(output, "<a href=\"");
		html_output_escape(output, name_basename);
		fprintf(output, "/");
		if ( html_index )
			html_output_escape(output, html_index);
		fprintf(output, "#");
		if ( options )
			html_output_escape_fragment(output, options);
		else
			html_output_escape_fragment(output, name_basename);
		fprintf(output, "\">");
	}
	html_output_escape(output, name_basename);
	if ( subsuite )
		fprintf(output, "</a>");
	fprintf(output, "</th>\n");

	for ( size_t o = 0; o < oss_count; o++ )
	{
		// Aggregate outcomes per their judgement.
		struct statistics* statistics = &statistics_for_os[o];
		size_t judgements[JUDGEMENT_MAX] = {0};
		for ( enum outcome outcome = 0; outcome < OUTCOME_MAX; outcome++ )
			judgements[judge(outcome)] += statistics->counters[outcome];
		size_t good = judgements[JUDGEMENT_GOOD];
		size_t partial = judgements[JUDGEMENT_PARTIAL];
		size_t bad = judgements[JUDGEMENT_BAD];
		size_t total = good + partial + bad;
		size_t percentage = total ? (good * 100) / total : 0;
		const char* os = oss[o];
		const char* css_class =
			!total ? "none" : good == total ? "good" : "bad";
		fprintf(output, "            <td class=\"");
		html_output_escape(output, css_class);
		fprintf(output, "\"");
		if ( total && good < total )
		{
			size_t green = total ? ((good + partial) * 255) / total : 0;
			if ( green < 128 )
				green = 128;
			fprintf(output, " style=\"background-color: rgb(255, %zu, 128)\"",
			        green);
		}
		fprintf(output, ">");
		html_output_escape(output, os);
		fprintf(output, ": ");
		fprintf(output, "<br>\n");
		fprintf(output, "              <span class=\"score\">");
		if ( subsuite )
		{
			fprintf(output, "<a href=\"");
			html_output_escape(output, name_basename);
			fprintf(output, "/");
			if ( html_index )
				html_output_escape(output, html_index);
			fprintf(output, "#");
			if ( options )
				html_output_escape_fragment(output, options);
			else
				html_output_escape_fragment(output, name_basename);
			fprintf(output, "\">");
		}
		fprintf(output, "%zu%%", percentage);
		if ( subsuite )
			fprintf(output, "</a>");
		fprintf(output, "</span><br>\n");
		fprintf(output, "              (%zu/%zu)\n", good, total);
		if ( partial )
		{
			percentage = total ? ((good + partial) * 100) / total : 0;
			fprintf(output, "              <small><br>~<br>\n");
			fprintf(output, "              %zu%% =\n", percentage);
			for ( enum outcome outcome = 0; outcome < OUTCOME_MAX; outcome++ )
			{
				if ( judge(outcome) == JUDGEMENT_PARTIAL &&
				     statistics->counters[outcome] )
				{
					size_t value = statistics->counters[outcome];
					percentage = total ? (value * 100) / total : 0;
					fprintf(output, "              <br>+%zu%% (%zu) as %s\n",
					        percentage, value, outcome_names[outcome]);
				}
			}
			fprintf(output, "              </small>\n");
		}
		fprintf(output, "</td>\n");
	}

	fprintf(output, "          </tr>\n");
}

static void html_output_table_end_options(FILE* output, const char* suite,
                                          const char* options)
{
	(void) output;
	(void) suite;
	(void) options;
}

static void html_output_table_end(FILE* output, const char* suite)
{
	(void) suite;
	fprintf(output, "        </tbody>\n");
	fprintf(output, "      </table>\n");
}

static FILE* html_output_begin_suite(FILE* global_output, const char* suite)
{
	if ( !output_directory )
		return global_output;
	char* output_dir =
		suite ? join_paths(output_directory, suite) : strdup(output_directory);
	if ( !output_dir )
		err(1, "malloc");
	if ( mkdir_p(output_dir, 0777) < 0 )
		err(1, "%s", output_dir);
	char* output_path = join_paths(output_dir, "index.html");
	if ( !output_path )
		err(1, "malloc");
	printf("> %s\n", output_path);
	FILE* output = fopen(output_path, "w");
	if ( !output )
		err(1, "%s", output_path);
	return output;
}

static void html_output_end_suite(FILE* global_output, FILE* output,
                                  const char* suite)
{
	(void) suite;
	if ( ferror(output) || fflush(output) == EOF )
		err(1, "writing output");
	if ( global_output != output && output != stdout )
		fclose(output);
}

static FILE* html_output_begin(void)
{
	if ( !output_file )
		return stdout;
	FILE* global_output = fopen(output_file, "w");
	if ( !global_output )
		err(1, "%s", output_file);
	return global_output;
}

static void html_output_end(FILE* global_output)
{
	if ( !global_output )
		return;
	if ( ferror(global_output) || fflush(global_output) == EOF )
		err(1, "writing output");
	if ( global_output != stdout )
		fclose(global_output);
}

static void (*output_header)(FILE* output);
static void (*output_footer)(FILE* output);
static void (*output_title)(FILE* output, const char* suite);
static void (*output_introduction)(FILE* output, const char* suite);
static void (*output_legend)(FILE* output, const char* suite);
static void (*output_legend_include)(FILE* output, const char* suite);
static void (*output_legend_namespace)(FILE* output, const char* suite);
static void (*output_legend_overview)(FILE* output, const char* suite);
static void (*output_suites)(FILE* output, const char* suite,
                             const char* const* subsuites,
                             size_t subsuites_count);
static void (*output_section)(FILE* output, const char* suite);
static void (*output_table_begin)(FILE* output, const char* suite);
static void (*output_table_begin_options)(FILE* output, const char* suite,
                                          const char* options);
static void (*output_results)(FILE* output, const char* suite,
                              const char* test, bool optional,
                              const char* options, char** result_for_os,
                              enum outcome* outcome_for_os,
                              char** expectations,
                              size_t expectations_count);
static void (*output_table_statistics)(FILE* output, const char* suite,
                                       const char* subsuite,
                                       const char* options,
                                       struct statistics* statistics_for_os);
static void (*output_table_end_options)(FILE* output, const char* suite,
                                        const char* options);
static void (*output_table_end)(FILE* output, const char* suite);
static FILE* (*output_begin_suite)(FILE* global_output, const char* suite);
static void (*output_end_suite)(FILE* global_output, FILE* output,
                                const char* suite);
static FILE* (*output_begin)(void);
static void (*output_end)(FILE* global_output);

static int filter_dotdot(const struct dirent* dirent)
{
	if ( !strcmp(dirent->d_name, ".") || !strcmp(dirent->d_name, "..") )
		return 0;
	return 1;
}

static int filter_source(const struct dirent* dirent)
{
	if ( !strcmp(dirent->d_name, ".") || !strcmp(dirent->d_name, "..") )
		return 0;
	size_t length = strlen(dirent->d_name);
	return 2 <= length && !strcmp(dirent->d_name + length - 2, ".c");
}

static int filter_source_and_header(const struct dirent* dirent)
{
	if ( !strcmp(dirent->d_name, ".") || !strcmp(dirent->d_name, "..") )
		return 0;
	size_t length = strlen(dirent->d_name);
	return 2 <= length &&
	       (!strcmp(dirent->d_name + length - 2, ".c") ||
	        !strcmp(dirent->d_name + length - 2, ".h"));
}

static const char* filter_subsuite_path = NULL;
static int filter_subsuite(const struct dirent* dirent)
{
	if ( !strcmp(dirent->d_name, ".") || !strcmp(dirent->d_name, "..") )
		return 0;
#ifdef DT_UNKNOWN
	if ( dirent->d_type != DT_UNKNOWN )
		return dirent->d_type == DT_DIR;
#endif
	char* path = join_paths(filter_subsuite_path, dirent->d_name);
	if ( !path )
		err(1, "malloc");
	struct stat st;
	if ( stat(path, &st) < 0 )
		err(1, "stat: %s", path);
	free(path);
	return S_ISDIR(st.st_mode);
}

static void output_sources(const char* input_dir, const char* subdir)
{
	char* output_dir = join_paths(output_directory, subdir);
	if ( !output_dir )
		err(1, "malloc");
	if ( mkdir_p(output_dir, 0777) < 0 )
		err(1, "%s", output_dir);
	struct dirent** entries;
	int count = scandir(input_dir, &entries, filter_source_and_header,
	                    no_extension_sort);
	if ( count < 0 )
		err(1, "%s", input_dir);
	for ( int i = 0; i < count; i++ )
	{
		char* input_path = join_paths(input_dir, entries[i]->d_name);
		char* output_path = join_paths(output_dir, entries[i]->d_name);
		if ( !input_path || !output_path )
			err(1, "malloc");
		char* data = read_text_file(input_path);
		if ( !data )
			err(1, "%s", input_path);
		FILE* fp = fopen(output_path, "w");
		if ( !fp )
			err(1, "%s", output_path);
		fputs(data, fp);
		if ( ferror(fp) || fflush(fp) == EOF )
			err(1, "write: %s", output_path);
		fclose(fp);
		free(data);
		free(input_path);
		free(output_path);
	}
	for ( int i = 0; i < count; i++ )
		free(entries[i]);
	free(entries);
	free(output_dir);
}

// Extract the options for the test and whether it's optional using source code
// comment annotations.
static void get_test_options(char* path, char** options, bool* optional)
{
	*options = NULL;
	*optional = false;
	FILE* fp = fopen(path, "r");
	if ( !fp )
		err(1, "%s", path);
	char* line = NULL;
	size_t line_size = 0;
	ssize_t line_length;
	while ( 0 < (line_length = getline(&line, &line_size, fp)) )
	{
		if ( line[line_length-1] == '\n' )
			line[--line_length] = '\0';
		if ( !strcmp(line, "/*optional*/") )
			*optional = true;
		else if ( 2*3+1 <= line_length && !strncmp(line, "/*[", 3) &&
		          !strncmp(line + line_length - 3, "]*/", 3) )
		{
			free(*options);
			if ( !(*options = strndup(line + 3, line_length - 6)) )
				err(1, "malloc");
		}
	}
	free(line);
	if ( ferror(fp) )
		err(1, "getline: %s", path);
	fclose(fp);
}

// Determine the outcome of the test result based on the expectations.
static enum outcome classify(const char* test, const char* result,
                             int expectations_count,
                             struct dirent** expectations_entries,
                             char** expectations)
{
	size_t test_length = strlen(test);
	bool found_expectation = false;
	for ( int i = 0; i < expectations_count; i++ )
	{
		const char* name = expectations_entries[i]->d_name;
		if ( !strncmp(name, test, test_length) && name[test_length] == '.' )
		{
			found_expectation = true;
			if ( !strcmp(result, expectations[i]) )
			{
				if ( !strncmp(name + test_length + 1, "unknown.",
				              strlen("unknown.")) )
					return UNKNOWN;
				else
					return GOOD;
				break;
			}
		}
	}
	return found_expectation ? BAD : UNRATED;
}

// Get the first N expectations for a test.
static size_t get_expectations(const char* test,
                               int expectations_count,
                               struct dirent** expectations_entries,
                               char** expectations,
                               char** output,
                               size_t output_size)
{
	size_t test_length = strlen(test);
	size_t n = 0;
	for ( int i = 0; i < expectations_count; i++ )
	{
		const char* name = expectations_entries[i]->d_name;
		if ( !strncmp(name, test, test_length) && name[test_length] == '.' )
			if ( n < output_size )
				output[n++] = expectations[i];
	}
	return n;
}

static void report(FILE* global_output, const char* suite,
                   char*** out_options_list, size_t* out_options_list_count,
                   struct statistics** out_statistics);

static void report_overview(FILE* global_output, const char* suite,
                            const char* const* subsuites, size_t subsuites_count,
                            char*** out_options_list,
                            size_t* out_options_list_count,
                            struct statistics** out_statistics)
{
	char** options_list = NULL;
	size_t options_list_count = 0;
	size_t options_list_length = 0;
	struct statistics* all_statistics = NULL;

	char*** subsuites_options_list =
		calloc(subsuites_count, sizeof(char**));
	size_t* subsuites_options_list_count =
		calloc(subsuites_count, sizeof(size_t));
	struct statistics** subsuites_statistics =
		calloc(subsuites_count, sizeof(struct statistics*));
	if ( !subsuites_options_list || !subsuites_options_list_count ||
	     !subsuites_statistics )
		err(1, "malloc");

	// Build the report for each subsuite and combine the per-option statistics
	// into the overall per-optional overview statistics.
	for ( size_t i = 0; i < subsuites_count; i++ )
	{
		char** subsuite_options_list;
		size_t subsuite_options_list_count;
		struct statistics* subsuite_statistics;

		report(global_output, subsuites[i], &subsuite_options_list,
		       &subsuite_options_list_count, &subsuite_statistics);

		for ( size_t j = 0; j < subsuite_options_list_count; j++ )
		{
			char* options = subsuite_options_list[j];
			bool found = false;
			size_t found_n = 0;
			for ( size_t n = 0; !found && n < options_list_count; n++ )
			{
				if ( (found = !strcmp_null(options, options_list[n])) )
					found_n = n;
			}
			if ( !found )
			{
				char* copy = options ? strdup(options) : NULL;
				if ( (options && !copy) ||
				     !array_add(&options_list, &options_list_count,
				                &options_list_length, copy) )
					err(1, "malloc");
				size_t new_size =
					options_list_count * oss_count * sizeof(struct statistics);
				if ( !(all_statistics = realloc(all_statistics, new_size)) )
					err(1, "malloc");
				found_n = options_list_count-1;
				memset(all_statistics + found_n * oss_count, 0,
				       oss_count * sizeof(struct statistics));
			}
			for ( size_t o = 0; o < oss_count; o++ )
			{
				struct statistics* in_statistics =
					&subsuite_statistics[j * oss_count + o];
				struct statistics* out_statistics =
					&all_statistics[found_n * oss_count + o];
				for ( enum outcome outcome = 0;
				      outcome < OUTCOME_MAX;
				      outcome++ )
					out_statistics->counters[outcome] +=
						in_statistics->counters[outcome];
			}
		}

		subsuites_options_list[i] = subsuite_options_list;
		subsuites_options_list_count[i] = subsuite_options_list_count;
		subsuites_statistics[i] = subsuite_statistics;
	}

	char** options_sorted = malloc(options_list_count * sizeof(char*));
	if ( !options_sorted )
		err(1, "malloc");
	memcpy(options_sorted, options_list, options_list_count * sizeof(char*));
	qsort(options_sorted, options_list_count, sizeof(char*),
	      strcmp_null_indirect);

	FILE* output = output_begin_suite(global_output, suite);

	if ( global_output != output )
	{
		output_header(output);
		output_title(output, suite);
		output_introduction(output, suite);
		output_legend_overview(output, suite);
	}

	output_suites(output, suite, subsuites, subsuites_count);
	output_section(output, suite);
	output_table_begin(output, suite);

	for ( size_t option_i = 0; option_i < options_list_count; option_i++ )
	{
		const char* options = options_sorted[option_i];
		size_t option_n = 0;
		for ( size_t n = 0; n < options_list_count; n++ )
		{
			if ( !strcmp_null(options, options_list[n]) )
			{
				option_n = n;
				break;
			}
		}

		output_table_begin_options(output, suite, options);

		// Only output an overall row if there are multiple subsuites to unify.
		size_t multiple_subsuites = 0;
		for ( size_t s = 0; s < subsuites_count; s++ )
		{
			for ( size_t n = 0; n < subsuites_options_list_count[s]; n++ )
			{
				if ( !strcmp_null(options, subsuites_options_list[s][n]) )
				{
					multiple_subsuites++;
					if ( 2 <= multiple_subsuites )
						break;
				}
			}
		}

		if ( 2 <= multiple_subsuites )
		{
			struct statistics* statistics_for_os =
				&all_statistics[option_n * oss_count];
			output_table_statistics(output, suite, NULL, options,
				                    statistics_for_os);
		}

		for ( size_t s = 0; s < subsuites_count; s++ )
		{
			const char* subsuite = subsuites[s];
			bool found = false;
			option_n = 0;
			for ( size_t n = 0; n < subsuites_options_list_count[s]; n++ )
			{
				if ( !strcmp_null(options, subsuites_options_list[s][n]) )
				{
					found = true;
					option_n = n;
					break;
				}
			}

			if ( found )
			{
				struct statistics* statistics_for_os =
					&subsuites_statistics[s][option_n * oss_count];
				output_table_statistics(output, suite, subsuite, options,
					                    statistics_for_os);
			}
		}
	}

	output_table_end(output, suite);

	if ( global_output != output )
		output_footer(output);

	output_end_suite(global_output, output, suite);

	free(options_sorted);

	for ( size_t i = 0; i < subsuites_count; i++ )
	{
		for ( size_t j = 0; j < subsuites_options_list_count[i]; j++ )
			free(subsuites_options_list[i][j]);
		free(subsuites_options_list[i]);
	}
	free(subsuites_options_list);
	free(subsuites_options_list_count);
	for ( size_t i = 0; i < subsuites_count; i++ )
		free(subsuites_statistics[i]);
	free(subsuites_statistics);

	*out_options_list = options_list;
	*out_options_list_count = options_list_count;
	*out_statistics = all_statistics;
}

static void report_suite(FILE* global_output, const char* suite,
                         char*** out_options_list,
                         size_t* out_options_list_count,
                         struct statistics** out_statistics)
{
	FILE* output = output_begin_suite(global_output, suite);

	if ( global_output != output )
		output_header(output);

	char* suite_expect;
	if ( format_string(&suite_expect, "%s.expect", suite) < 0 )
		err(1, "malloc");
	char* suite_path = join_paths(suites_directory, suite);
	if ( !suite_path )
		err(1, "malloc");

	if ( output_directory )
		output_sources(suite_path, suite);

	// Compute the list of test source files.
	struct dirent** entries;
	int count = scandir(suite_path, &entries, filter_source, no_extension_sort);
	if ( count < 0 )
		err(1, "%s", suite_path);

	// Load the expectations if they exist. If not, then .out files contain the
	// test outcomes directly.
	char* expectations_path = join_paths(expectations_directory, suite_expect);
	if ( !suite_path || ! expectations_path )
		err(1, "malloc");
	bool has_expectations = !access(expectations_path, F_OK);
	if ( !has_expectations && errno != ENOENT )
		err(1, "%s", expectations_path);
	int expectations_count = 0;
	struct dirent** expectations_entries = NULL;
	char** expectations = NULL;
	if ( has_expectations )
	{
		expectations_count = scandir(expectations_path, &expectations_entries,
		                             filter_dotdot, no_extension_sort);
		if ( expectations_count < 0 )
			err(1, "%s", expectations_path);
		if ( !(expectations = calloc(expectations_count, sizeof(char*))) )
			err(1, "malloc");
		for ( int i = 0; i < expectations_count; i++ )
		{
			char* path = join_paths(expectations_path,
			                        expectations_entries[i]->d_name);
			if ( !path )
				err(1, "malloc");
			if ( !(expectations[i] = read_text_file(path)) )
				err(1, "%s", path);
			free(path);
		}
	}

	// Determine which options are used by the tests per the source files.
	char** options_list = NULL;
	size_t options_list_count = 0;
	size_t options_list_length = 0;
	for ( int i = 0; i < count; i++ )
	{
		char* test_path = join_paths(suite_path, entries[i]->d_name);
		if ( !test_path )
			err(1, "malloc");
		char* options;
		bool optional = false;
		get_test_options(test_path, &options, &optional);

		bool found = false;
		for ( size_t n = 0; !found && n < options_list_count; n++ )
			found = !strcmp_null(options, options_list[n]);
		if ( found )
			free(options);
		else if ( !array_add(&options_list, &options_list_count,
		                     &options_list_length, options) )
			err(1, "malloc");

		free(test_path);
	}
	qsort(options_list, options_list_count, sizeof(char*),
	      strcmp_null_indirect);

	// Allocate the per-option statistics and per-os row information.
	struct statistics* all_statistics =
		calloc(oss_count, sizeof(struct statistics) * options_list_count);
	char** result_for_os = calloc(oss_count, sizeof(char*));
	enum outcome* outcome_for_os = calloc(oss_count, sizeof(enum outcome));
	if ( !all_statistics || !result_for_os || !outcome_for_os )
		err(1, "malloc");

	if ( global_output != output )
	{
		output_title(output, suite);
		if ( has_expectations )
			output_legend(output, suite);
		else if ( !strcmp(suite, "namespace") )
			output_legend_namespace(output, suite);
		else
			output_legend_include(output, suite);
	}
	output_section(output, suite);
	output_table_begin(output, suite);

	// Slice the test results into per-option table sections.
	for ( size_t option_i = 0; option_i < options_list_count; option_i++ )
	{
		const char* options = options_list[option_i];

		output_table_begin_options(output, suite, options);

		for ( int i = 0; i < count; i++ )
		{
			char* test_path = join_paths(suite_path, entries[i]->d_name);
			if ( !test_path )
				err(1, "malloc");
			char* test_options;
			bool test_optional = false;
			get_test_options(test_path, &test_options, &test_optional);
			free(test_path);
			if ( !((!test_options && !options) ||
			       (test_options && options &&
			        !strcmp(test_options, options))) )
			{
				free(test_options);
				continue;
			}
			char* test =
				strndup(entries[i]->d_name, strcspn(entries[i]->d_name, "."));
			if ( !test_path )
				err(1, "malloc");
			char* test_out;
			if ( format_string(&test_out, "%s.out", test) < 0 )
				err(1, "malloc");

			// Build the test results for each OS on the test.
			for ( size_t o = 0; o < oss_count; o++ )
			{
				const char* os = oss[o];
				char* result_os_dir = join_paths(os_directory, os);
				if ( !result_os_dir )
					err(1, "malloc");
				char* result_suite_dir = join_paths(result_os_dir, suite);
				if ( !result_suite_dir )
					err(1, "malloc");
				char* result_path = join_paths(result_suite_dir, test_out);
				if ( !result_path )
					err(1, "malloc");
				FILE* result_fp = fopen(result_path, "r");
				if ( !result_fp && errno != ENOENT )
					err(1, "%s", result_path);
				else if ( !result_fp )
				{
					result_for_os[o] = NULL;
					outcome_for_os[o] = NONE;
				}
				else
				{
					char* result = NULL;
					size_t size = 0;
					if ( getdelim(&result, &size, '\0', result_fp) < 0 )
						err(1, "read: %s", result_path);
					size_t length = strlen(result);
					bool had_newline = length && result[length - 1] == '\n';
					if ( had_newline )
						result[length - 1] = '\0';
					result_for_os[o] = NULL;
					outcome_for_os[o] = outcome_parse(result);
					if ( had_newline )
						result[length - 1] = '\n';
					if ( outcome_for_os[o] != OUTCOME_MAX )
						free(result);
					else if ( has_expectations )
					{
						result_for_os[o] = result;
						outcome_for_os[o] =
							classify(test, result, expectations_count,
								     expectations_entries, expectations);
					}
					else
					{
						result_for_os[o] = result;
						outcome_for_os[o] =
							!strcmp(result, "exit: 0\n") ? GOOD : BAD;
					}
					struct statistics* statistics =
						&all_statistics[option_i * oss_count + o];
					statistics->counters[outcome_for_os[o]]++;
					fclose(result_fp);
				}
				free(result_path);
				free(result_suite_dir);
				free(result_os_dir);
			}

			char* test_expectations[5];
			size_t test_expectations_count =
				get_expectations(test, expectations_count, expectations_entries,
				                 expectations, test_expectations, 5);
			output_results(output, suite, test, test_optional, test_options,
				           result_for_os, outcome_for_os, test_expectations,
			               test_expectations_count);
			for ( size_t o = 0; o < oss_count; o++ )
				free(result_for_os[o]);

			free(test_options);
			free(test);
			free(test_out);
		}

		output_table_end_options(output, suite, options);
	}

	output_table_end(output, suite);

	free(result_for_os);
	free(outcome_for_os);

	for ( int i = 0; i < expectations_count; i++ )
	{
		free(expectations[i]);
		free(expectations_entries[i]);
	}
	free(expectations_entries);
	free(expectations_path);
	free(expectations);

	for ( int i = 0; i < count; i++ )
		free(entries[i]);
	free(entries);
	free(suite_path);
	free(suite_expect);

	if ( global_output != output )
		output_footer(output);

	output_end_suite(global_output, output, suite);

	*out_options_list = options_list;
	*out_options_list_count = options_list_count;
	*out_statistics = all_statistics;
}

static void report(FILE* global_output, const char* suite,
                   char*** out_options_list, size_t* out_options_list_count,
                   struct statistics** out_statistics)
{
	char* suite_path = join_paths(suites_directory, suite);
	if ( !suite_path )
		err(1, "malloc");
	struct dirent** entries;
	filter_subsuite_path = suite_path;
	int count = scandir(suite_path, &entries, filter_subsuite, alphasort);
	if ( count < 0 )
		err(1, "%s", suite_path);
	if ( count )
	{
		char** subsuites = calloc(count, sizeof(char*));
		if ( !subsuites )
			err(1, "malloc");
		for ( int i = 0; i < count; i++ )
		{
			if ( !(subsuites[i] = join_paths(suite, entries[i]->d_name)) )
				err(1, "malloc");
		}
		report_overview(global_output, suite, (const char* const*) subsuites,
		                count, out_options_list, out_options_list_count,
		                out_statistics);
		for ( int i = 0; i < count; i++ )
			free(subsuites[i]);
		free(subsuites);
	}
	else
	{
		report_suite(global_output, suite, out_options_list,
		             out_options_list_count, out_statistics);
	}
	for ( int i = 0; i < count; i++ )
		free(entries[i]);
	free(entries);
	free(suite_path);
}
 
static void compact_arguments(int* argc, char*** argv)
{
	for ( int i = 0; i < *argc; i++ )
	{
		while ( i < *argc && !(*argv)[i] )
		{
			for ( int n = i; n < *argc; n++ )
				(*argv)[n] = (*argv)[n+1];
			(*argc)--;
		}
	}
}

int main(int argc, char* argv[])
{
	char* format = "html";
	char* os_list = NULL;
	char* suites_list = NULL;

	// Make sure all strings are sorted deterministically per C locale rules.
	setlocale(LC_ALL, "C");

	// Parse options manually as getopt_long has not been standardized.
	for ( int i = 1; i < argc; i++ )
	{
		char* arg = argv[i];
		if ( arg[0] != '-' || !arg[1] )
			continue;
		argv[i] = NULL;
		if ( !strcmp(arg, "--") )
			break;
		if ( arg[1] != '-' )
		{
			char c;
			while ( (c = *++arg) ) switch ( c )
			{
			default:
				errx(1, "unknown option -- '%c'", c);
			}
		}
		else if ( !strncmp(arg, "--expectations-directory=",
		          strlen("--expectations-directory=")) )
			expectations_directory = arg + strlen("--expectations-directory=");
		else if ( !strcmp(arg, "--expectations-directory") )
		{
			if ( i + 1 == argc )
				errx(125, "option '%s' requires an argument", arg);
			expectations_directory = argv[i+1];
			argv[++i] = NULL;
		}
		else if ( !strncmp(arg, "--format=", strlen("--format=")) )
			format = arg + strlen("--format=");
		else if ( !strcmp(arg, "--format") )
		{
			if ( i + 1 == argc )
				errx(125, "option '%s' requires an argument", arg);
			format = argv[i+1];
			argv[++i] = NULL;
		}
		else if ( !strncmp(arg, "--html-footer=", strlen("--html-footer=")) )
			html_footer = arg + strlen("--html-footer=");
		else if ( !strcmp(arg, "--html-footer") )
		{
			if ( i + 1 == argc )
				errx(125, "option '%s' requires an argument", arg);
			html_footer = argv[i+1];
			argv[++i] = NULL;
		}
		else if ( !strncmp(arg, "--html-header=", strlen("--html-header=")) )
			html_header = arg + strlen("--html-header=");
		else if ( !strcmp(arg, "--html-header") )
		{
			if ( i + 1 == argc )
				errx(125, "option '%s' requires an argument", arg);
			html_header = argv[i+1];
			argv[++i] = NULL;
		}
		else if ( !strncmp(arg, "--html-index=", strlen("--html-index=")) )
			html_index = arg + strlen("--html-index=");
		else if ( !strcmp(arg, "--html-index") )
		{
			if ( i + 1 == argc )
				errx(125, "option '%s' requires an argument", arg);
			html_index = argv[i+1];
			argv[++i] = NULL;
		}
		else if ( !strncmp(arg, "--html-introduction=",
		          strlen("--html-introduction=")) )
			html_introduction = arg + strlen("--html-introduction=");
		else if ( !strcmp(arg, "--html-introduction") )
		{
			if ( i + 1 == argc )
				errx(125, "option '%s' requires an argument", arg);
			html_introduction = argv[i+1];
			argv[++i] = NULL;
		}

		else if ( !strncmp(arg, "--html-legend=",
		          strlen("--html-legend=")) )
			html_legend = arg + strlen("--html-legend=");
		else if ( !strcmp(arg, "--html-legend") )
		{
			if ( i + 1 == argc )
				errx(125, "option '%s' requires an argument", arg);
			html_legend = argv[i+1];
			argv[++i] = NULL;
		}
		else if ( !strncmp(arg, "--html-legend-include=",
		          strlen("--html-legend-include=")) )
			html_legend_include = arg + strlen("--html-legend-include=");
		else if ( !strcmp(arg, "--html-legend-include") )
		{
			if ( i + 1 == argc )
				errx(125, "option '%s' requires an argument", arg);
			html_legend_include = argv[i+1];
			argv[++i] = NULL;
		}
		else if ( !strncmp(arg, "--html-legend-namespace=",
		          strlen("--html-legend-namespace=")) )
			html_legend_namespace = arg + strlen("--html-legend-namespace=");
		else if ( !strcmp(arg, "--html-legend-namespace") )
		{
			if ( i + 1 == argc )
				errx(125, "option '%s' requires an argument", arg);
			html_legend_namespace = argv[i+1];
			argv[++i] = NULL;
		}
		else if ( !strncmp(arg, "--html-legend-overview=",
		          strlen("--html-legend-overview=")) )
			html_legend_overview = arg + strlen("--html-legend-overview=");
		else if ( !strcmp(arg, "--html-legend-overview") )
		{
			if ( i + 1 == argc )
				errx(125, "option '%s' requires an argument", arg);
			html_legend_overview = argv[i+1];
			argv[++i] = NULL;
		}
		else if ( !strncmp(arg, "--os-directory=", strlen("--os-directory=")) )
			os_directory = arg + strlen("--os-directory=");
		else if ( !strcmp(arg, "--os-directory") )
		{
			if ( i + 1 == argc )
				errx(125, "option '%s' requires an argument", arg);
			os_directory = argv[i+1];
			argv[++i] = NULL;
		}
		else if ( !strncmp(arg, "--os-list=", strlen("--os-list=")) )
			os_list = arg + strlen("--os-list=");
		else if ( !strcmp(arg, "--os-list") )
		{
			if ( i + 1 == argc )
				errx(125, "option '%s' requires an argument", arg);
			os_list = argv[i+1];
			argv[++i] = NULL;
		}
		else if ( !strncmp(arg, "--output=", strlen("--output=")) )
			output_file = arg + strlen("--output=");
		else if ( !strcmp(arg, "--output") )
		{
			if ( i + 1 == argc )
				errx(125, "option '%s' requires an argument", arg);
			output_file = argv[i+1];
			argv[++i] = NULL;
		}
		else if ( !strncmp(arg, "--output-directory=",
		          strlen("--output-directory=")) )
			output_directory = arg + strlen("--output-directory=");
		else if ( !strcmp(arg, "--output-directory") )
		{
			if ( i + 1 == argc )
				errx(125, "option '%s' requires an argument", arg);
			output_directory = argv[i+1];
			argv[++i] = NULL;
		}
		else if ( !strcmp(arg, "--shorten-results") )
			shorten_results = true;
		else if ( !strncmp(arg, "--suites-list=", strlen("--suites-list=")) )
			suites_list = arg + strlen("--suites-list=");
		else if ( !strcmp(arg, "--suites-list") )
		{
			if ( i + 1 == argc )
				errx(125, "option '%s' requires an argument", arg);
			suites_list = argv[i+1];
			argv[++i] = NULL;
		}
		else if ( !strncmp(arg, "--suites-directory=",
		          strlen("--suites-directory=")) )
			suites_directory = arg + strlen("--suites-directory=");
		else if ( !strcmp(arg, "--suites-directory") )
		{
			if ( i + 1 == argc )
				errx(125, "option '%s' requires an argument", arg);
			suites_directory = argv[i+1];
			argv[++i] = NULL;
		}
		else
			errx(1, "unknown option: %s", arg);
	}

	compact_arguments(&argc, &argv);
	
	if ( 1 < argc )
			errx(1, "unexpected extra operand: %s", argv[1]);

	if ( !strcmp(format, "html") )
	{
		output_header = html_output_header;
		output_footer = html_output_footer;
		output_title = html_output_title;
		output_introduction = html_output_introduction;
		output_legend = html_output_legend;
		output_legend_include = html_output_legend_include;
		output_legend_namespace = html_output_legend_namespace;
		output_legend_overview = html_output_legend_overview;
		output_suites = html_output_suites;
		output_section = html_output_section;
		output_table_begin = html_output_table_begin;
		output_table_begin_options = html_output_table_begin_options;
		output_results = html_output_results;
		output_table_statistics = html_output_table_statistics;
		output_table_end_options = html_output_table_end_options;
		output_table_end = html_output_table_end;
		output_begin_suite = html_output_begin_suite;
		output_end_suite = html_output_end_suite;
		output_begin = html_output_begin;
		output_end = html_output_end;
	}
	else if ( !strcmp(format, "json") || !strcmp(format, "jsonl") )
	{
		json_lines = !strcmp(format, "jsonl");
		output_header = json_output_header;
		output_footer = json_output_footer;
		output_title = json_output_title;
		output_introduction = json_output_introduction;
		output_legend = json_output_legend;
		output_legend_include = json_output_legend_include;
		output_legend_namespace = json_output_legend_namespace;
		output_legend_overview = json_output_legend_overview;
		output_suites = json_output_suites;
		output_section = json_output_section;
		output_table_begin = json_output_table_begin;
		output_table_begin_options = json_output_table_begin_options;
		output_results = json_output_results;
		output_table_statistics = json_output_table_statistics;
		output_table_end_options = json_output_table_end_options;
		output_table_end = json_output_table_end;
		output_begin_suite = json_output_begin_suite;
		output_end_suite = json_output_end_suite;
		output_begin = json_output_begin;
		output_end = json_output_end;
	}
	else
		errx(1, "unknown output format: %s", format);

	// Compute the effective paths per the options and default values.
	if ( !html_index )
		html_index = "index.html";
	if ( !suites_directory )
		suites_directory = ".";
	if ( !expectations_directory )
		expectations_directory = suites_directory;
	char* misc_path = join_paths(suites_directory, "misc");
	if ( !misc_path )
		err(1, "malloc");
	if ( !os_directory )
		os_directory = join_paths(suites_directory, "out");
	if ( !html_footer )
		html_footer = join_paths(misc_path, "footer.html");
	if ( !html_header )
		html_header = join_paths(misc_path, "header.html");
	if ( !html_legend )
		html_legend = join_paths(misc_path, "legend.html");
	if ( !html_legend_include )
		html_legend_include = join_paths(misc_path, "legend.include.html");
	if ( !html_legend_namespace )
		html_legend_namespace = join_paths(misc_path, "legend.namespace.html");
	if ( !html_legend_overview )
		html_legend_overview = join_paths(misc_path, "legend.overview.html");
	if ( !os_directory || !html_footer || !html_header || !html_legend ||
	     !html_legend_include || !html_legend_namespace || !html_legend_overview )
		err(1, "malloc");

	if ( !output_file && !output_directory && !strcmp(format, "html") &&
		 !(output_directory = join_paths(suites_directory, "html")) )
		err(1, "malloc");

	// Determine the list of operating systems to report results for.
	if ( os_list )
	{
		char* str = os_list;
		char* save = NULL;
		char* os;
		while ( (os = strtok_r(str, " \t\n", &save)) )
		{
			char* copy = strdup(os);
			if ( !copy ||
			     !array_add(&oss, &oss_count, &oss_length, copy) )
				err(1, "malloc");
			str = NULL;
		}
	}
	else
	{
		struct dirent** entries;
		int count = scandir(os_directory, &entries, filter_dotdot, alphasort);
		if ( count < 0 )
			err(1, "%s", os_directory);
		for ( int i = 0; i < count; i++ )
		{
			char* copy = strdup(entries[i]->d_name);
			if ( !copy ||
			     !array_add(&oss, &oss_count, &oss_length, copy) )
				err(1, "malloc");
			free(entries[i]);
		}
		free(entries);
	}

	// Build the list of suites to report results for.
	char** suites = NULL;
	size_t suites_count = 0;
	size_t suites_length = 0;
	if ( suites_list )
	{
		char* str = suites_list;
		char* save = NULL;
		char* suite;
		while ( (suite = strtok_r(str, " \t\n", &save)) )
		{
			char* copy = strdup(suite);
			if ( !copy ||
			     !array_add(&suites, &suites_count, &suites_length,
			                copy) )
				err(1, "malloc");
			str = NULL;
		}
	}
	else
	{
		char* suites_list_path = join_paths(misc_path, "suites.list");
		if ( !suites_list_path )
			err(1, "malloc");
		FILE* fp = fopen(suites_list_path, "r");
		if ( !fp )
			err(1, "%s", suites_list_path);
		char* line = NULL;
		size_t line_size = 0;
		ssize_t line_length;
		while ( 0 < (line_length = getline(&line, &line_size, fp)) )
		{
			if ( line[line_length-1] == '\n' )
				line[--line_length] = '\0';
			if ( !array_add(&suites, &suites_count, &suites_length,
				            line) )
				err(1, "malloc");
			line = NULL;
		}
		free(line);
		if ( ferror(fp) )
			err(1, "getline: %s", suites_list_path);
		fclose(fp);
		free(suites_list_path);
	}
	qsort(suites, suites_count, sizeof(char*), strcmp_null_indirect);

	FILE* global_output = output_begin();

	if ( global_output && !output_directory )
	{
		output_header(global_output);
		output_title(global_output, NULL);
		output_introduction(global_output, NULL);
	}

	// Output the os-test report with test results and statistics.
	char** options_list;
	size_t options_list_count;
	struct statistics* all_statistics;
	report_overview(global_output, NULL, (const char* const*) suites,
	                suites_count, &options_list, &options_list_count,
	                &all_statistics);
	if ( output_directory )
		output_sources(misc_path, "misc");
	for ( size_t i = 0; i < options_list_count; i++ )
		free(options_list[i]);
	free(options_list);
	free(all_statistics);

	if ( global_output && !output_directory )
		output_footer(global_output);

	output_end(global_output);

	for ( size_t i = 0; i < suites_count; i++ )
		free(suites[i]);
	free(suites);

	free(misc_path);

	return 0;
}
