/* Test whether a basic FD_ZERO invocation works. */

#include <sys/select.h>

#include "../basic.h"

int main(void)
{
	fd_set fdset;
	FD_ZERO(&fdset);
	for ( int i = 0; i < FD_SETSIZE; i++ )
		if ( FD_ISSET(i, &fdset) )
			errx(1, "FD_ZERO did not zero");
	return 0;
}
