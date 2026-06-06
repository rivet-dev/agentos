/* Test whether a basic fnmatch invocation works. */

#include <fnmatch.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	int ret;
	const char* pattern;
	const char* input;

	// Test a pattern using * that doesn't match.
	pattern = "foo*bar";
	input = "foobarx";
	ret = fnmatch(pattern, input, 0);
	if ( ret == 0 )
		errx(1, "fnmatch \"%s\" should not match \"%s\"", pattern, input);
	else if ( ret != FNM_NOMATCH )
		errx(1, "fnmatch failed weirdly");

	// Test a pattern using * that does match.
	pattern = "foo*bar";
	input = "foodbar";
	ret = fnmatch(pattern, input, 0);
	if ( ret == FNM_NOMATCH )
		errx(1, "fnmatch \"%s\" should match \"%s\"", pattern, input);
	else if ( ret != 0 )
		errx(1, "fnmatch failed weirdly");

	// Test [ expressions that match.
	pattern = "fo[o/][o/][!bar]bar";
	input = "foo//bar";
	ret = fnmatch(pattern, input, 0);
	if ( ret == FNM_NOMATCH )
		errx(1, "fnmatch \"%s\" should match \"%s\"", pattern, input);
	else if ( ret != 0 )
		errx(1, "fnmatch failed weirdly");

	// Test [ expressions where they don't match due to FNM_PATHNAME.
	pattern = "fo[o/][o/][!bar]bar";
	input = "foo//bar";
	ret = fnmatch(pattern, input, FNM_PATHNAME);
	if ( ret == 0 )
		errx(1, "fnmatch \"%s\" FNM_PATHNAME should not match \"%s\"",
		     pattern, input);
	else if ( ret != FNM_NOMATCH )
		errx(1, "fnmatch failed weirdly");

	// Test whether * is implemented efficiently or not.
	pattern = "********************a";
	input = "xxxxxxxxxxxxxxxxxxxxb";
	alarm(1);
	ret = fnmatch(pattern, input, 0);
	alarm(0);
	if ( ret == 0 )
		errx(1, "fnmatch \"%s\" should not match \"%s\"", pattern, input);
	else if ( ret != FNM_NOMATCH )
		errx(1, "fnmatch failed weirdly");

	return 0;
}
