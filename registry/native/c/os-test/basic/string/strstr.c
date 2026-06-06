/* Test whether a basic strstr invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	const char haystack[] = "haystack";
	char* ptr = strstr(haystack, "sta");
	if ( !ptr )
		errx(1, "strstr was NULL");
	if ( ptr != haystack + 3 )
		errx(1, "strstr found wrong needle");
	return 0;
}
