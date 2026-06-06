/* Test whether a basic memmem invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	const char haystack[] = "hayst\0ack";
	void* ptr = memmem(haystack, sizeof(haystack), "st\0a", 4);
	if ( !ptr )
		errx(1, "memmem was NULL");
	if ( ptr != haystack + 3 )
		errx(1, "memmem found wrong needle");
	return 0;
}
