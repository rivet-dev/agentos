/* Test whether a basic wordexp invocation works. */

#include <stdlib.h>
#include <string.h>
#include <wordexp.h>

#include "../basic.h"

int main(void)
{
	if ( setenv("FOO", "bar qux", 1) < 0 )
		err(1, "setenv");
	wordexp_t we = { .we_offs = 3 };
	int ret = wordexp("foo bar $FOO \"$FOO\" `echo $FOO`", &we, WRDE_DOOFFS);
	if ( ret == WRDE_BADCHAR )
		errx(1, "bad character");
	else if ( ret == WRDE_BADVAL )
		errx(1, "undefined variable");
	else if ( ret == WRDE_CMDSUB )
		errx(1, "denied command execution");
	else if ( ret == WRDE_CMDSUB )
		errx(1, "out of memoey");
	else if ( ret != 0 )
		errx(1, "wordexp failed weirdly");
	// Test if we_offs is respected.
	if ( we.we_offs != 3 )
		errx(1, "we_offs != 3");
	for ( size_t i = 0; i < we.we_offs; i++ )
		if ( we.we_wordv[i] )
			errx(1, "we_offs not respected");
	const char* expected[] =
	{
		"foo",
		"bar",
		"bar",
		"qux",
		"bar qux",
		"bar",
		"qux",
	};
	size_t expected_count = sizeof(expected) / sizeof(expected[0]);
	if ( we.we_wordc != expected_count )
		errx(1, "word count is %zu, not %zu", we.we_wordc, expected_count);
	for ( size_t i = 0; i < we.we_wordc; i++ )
	{
		const char* word = we.we_wordv[we.we_offs + i];
		if ( strcmp(word, expected[i]) != 0 )
			errx(1, "word %zu is \"%s\" not \"%s\"", i, word, expected[i]);
	}
	if ( we.we_wordv[we.we_offs + we.we_wordc] )
		errx(1, "wordexp did not null terminate word list");
	return 0;
}
