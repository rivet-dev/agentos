/* Test whether a basic nl_langinfo invocation works. */

#include <langinfo.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	char* output = nl_langinfo(MON_1);
	const char* expected = "January";
	if ( !output )
		err(1, "nl_langinfo MON_1");
	if ( !output[0] )
		errx(1, "nl_langinfo MON_1 = \"\"");
	if ( strcmp(output, expected) != 0 )
		errx(1, "got \"%s\" instead of \"%s\"", output, expected);
	return 0;
}
