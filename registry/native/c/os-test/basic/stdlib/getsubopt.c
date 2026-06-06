/* Test whether a basic getsubopt invocation works. */

#include <stdlib.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	char* const keys[] =
	{
		"foo",
		"bar",
		"food",
		"baz",
		NULL,
	};
	char orig_options[] = "food=heyyo,ba,ba=r,food";
	char* options = orig_options;
	char* value = NULL;
	int result = getsubopt(&options, keys, &value);
	if ( result != 2 )
		errx(1, "getsubopt did not return 2");
	if ( options != orig_options + 11 )
		errx(1, "getsubopt had wrong options offset");
	if ( !value )
		errx(1, "getsubopt had null value");
	if ( value != orig_options + 5 )
		errx(1, "getsubopt had wrong value offset");
	if ( strcmp(value, "heyyo") != 0 )
		errx(1, "getsubop had wrong value: %s", value);
	return 0;
}
