/* Test whether a basic fclose invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	if ( fclose(stdout) != 0 )
		err(1, "fclose");
	return 0;
}
