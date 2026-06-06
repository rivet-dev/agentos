/* Test whether a basic confstr invocation works. */

#include <stdlib.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	size_t needed = confstr(_CS_PATH, NULL, 0);
	if ( !needed )
		err(1, "first confstr");
	char* buffer = malloc(needed);
	if ( !buffer )
		err(1, "malloc");
	size_t result = confstr(_CS_PATH, buffer, needed);
	if ( !result )
		err(1, "second confstr");
	if ( result != needed )
		err(1, "second confstr returned wrong size");
	return 0;
}
