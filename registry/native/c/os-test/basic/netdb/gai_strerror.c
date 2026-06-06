/* Test whether a basic gai_strerror invocation works. */

#include <netdb.h>

#include "../basic.h"

int main(void)
{
	const char* str = gai_strerror(EAI_AGAIN);
	if ( !str )
		errx(1, "gai_strerror failed");
	if ( !str[0] )
		errx(1, "gai_strerror returned empty string");
	return 0;
}
