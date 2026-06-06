/* Test whether a basic strcasecmp invocation works. */

#include <strings.h>

#include "../basic.h"

int main(void)
{
	if ( strcasecmp("foo", "FOO") != 0 )
		errx(1, "strcasecmp(\"foo\", \"FOO\") weren't equal");
	return 0;
}
