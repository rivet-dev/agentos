/* Test whether a basic ctermid invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	char buffer[L_ctermid];
	char* result = ctermid(buffer);
	if ( !result )
		errx(1, "ctermid returned NULL");
	return 0;
}
