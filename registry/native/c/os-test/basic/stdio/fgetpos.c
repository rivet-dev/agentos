/* Test whether a basic fgetpos invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	size_t amount = fwrite("foo", 1, 3, fp);
	if ( amount != 3 )
		err(1, "fwrite");
	fpos_t pos;
	if ( fgetpos(fp, &pos) )
		err(1, "fgetpos");
	return 0;
}
