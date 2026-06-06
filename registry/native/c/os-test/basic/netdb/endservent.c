/* Test whether a basic endservent invocation works. */

#include <netdb.h>

#include "../basic.h"

int main(void)
{
	setservent(1);
	endservent();
	return 0;
}
