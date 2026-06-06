/* Test whether a basic fwrite invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	size_t amount = fwrite("foo", 1, 4, fp);
	if ( amount != 4 )
		err(1, "fwrite");
	return 0;
}
