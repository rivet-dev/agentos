/* Test whether a basic strerror_r invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	char buffer[256];
	if ( (errno = strerror_r(EILSEQ, buffer, sizeof(buffer)) < 0) )
	{
		if ( errno != ERANGE )
			err(1, "strerror_r");
	}
	return 0;
}
