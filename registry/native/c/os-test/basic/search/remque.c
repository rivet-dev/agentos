/*[XSI]*/
/* Test whether a basic remque invocation works. */

#include <search.h>

#include "../basic.h"

struct object
{
	struct object* next;
	struct object* prev;
};

int main(void)
{
	struct object a, b, c;
	a.next = &b;
	a.prev = NULL;
	b.next = &c;
	b.prev = &a;
	c.next = NULL;
	c.prev = &b;
	remque(&b);
	if ( a.next != &c )
		errx(1, "first remque: a.next != &c");
	if ( a.prev != NULL )
		errx(1, "first remque: a.prev != NULL");
	if ( c.next != NULL )
		errx(1, "first remque: c.next != NULL");
	if ( c.prev != &a )
		errx(1, "first remque: c.prev != &a");
	remque(&c);
	if ( a.next != NULL )
		errx(1, "second remque: a.next != NULL");
	if ( a.prev != NULL )
		errx(1, "second remque: a.prev != NULL");
	remque(&a);
	return 0;
}
