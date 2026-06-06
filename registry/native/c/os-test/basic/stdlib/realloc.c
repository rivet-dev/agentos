/* Test whether a basic realloc invocation works. */

#include <stdlib.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	char* buffer = malloc(8);
	if ( !buffer )
		err(1, "malloc");
	buffer[0] = 'a';
	buffer[1] = 'b';
	for ( size_t i = 2; i < 8; i++ )
		buffer[i] = 'a' + (buffer[i-2] - 'a') + (buffer[i-1] - 'a');
	buffer = realloc(buffer, sizeof("abbcdfin=foobar"));
	if ( !buffer )
		err(1, "realloc");
	strcpy(buffer + 8, "=foobar");
	if ( strcmp(buffer,"abbcdfin=foobar") != 0 )
		err(1, "incorrect: got %s wanted %s", buffer, "abbcdfin=foobar");
	return 0;
}
