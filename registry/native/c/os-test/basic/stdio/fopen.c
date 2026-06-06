/* Test whether a basic fopen invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = fopen("stdio/fopen", "r");
	if ( !fp )
		err(1, "fopen: stdio/fopen");
	return 0;
}
