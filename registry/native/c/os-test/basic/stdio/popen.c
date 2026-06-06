/* Test whether a basic popen invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = popen("echo foo", "r");
	if ( !fp )
		err(1, "popen");
	char buf[16];
	if ( !fgets(buf, sizeof(buf), fp) )
		err(1, "fgets");
	const char* expected = "foo\n";
	if ( strcmp(buf, expected) != 0 )
		err(1, "popen gave '%s' instead of '%s'", buf, expected);
	return 0;
}
