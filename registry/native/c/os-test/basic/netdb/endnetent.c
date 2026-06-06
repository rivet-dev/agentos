/* Test whether a basic endnetent invocation works. */

#include <netdb.h>

#include "../basic.h"

int main(void)
{
	setnetent(1);
	endnetent();
	return 0;
}
