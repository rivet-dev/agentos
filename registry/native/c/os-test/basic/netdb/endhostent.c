/* Test whether a basic endhostent invocation works. */

#include <netdb.h>

#include "../basic.h"

int main(void)
{
	sethostent(1);
	endhostent();
	return 0;
}
