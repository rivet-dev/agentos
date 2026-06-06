/* Test whether a basic endprotoent invocation works. */

#include <netdb.h>

#include "../basic.h"

int main(void)
{
	setprotoent(1);
	endprotoent();
	return 0;
}
