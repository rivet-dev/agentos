/* Test whether a basic memmove invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	char buf[8] = "abcdefg";

	// Test forward memmove.
	void* ptr = memmove(buf + 1, buf, 4);
	if ( ptr != buf + 1 )
		errx(1, "forward memmove did not return dst");
	const char* expected = "aabcdfg";
	if ( strcmp(buf, expected) != 0 )
		errx(1, "forward memmove gave %s instead of %s", buf, expected);

	// Test backward memmove.
	ptr = memmove(buf + 3, buf + 4, 4);
	if ( ptr != buf + 3 )
		errx(1, "backward memmove did not return dst");
	expected = "aabdfg";
	if ( strcmp(buf, expected) != 0 )
		errx(1, "backward memmove gave %s instead of %s", buf, expected);
	return 0;
}
