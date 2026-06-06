/*[XSI]*/
/* Test whether a basic insque invocation works. */

#include <search.h>

#include "../basic.h"

struct object
{
	struct object* next;
	struct object* prev;
};

int main(void)
{
	struct object a;
	insque(&a, NULL);
	if ( a.next != NULL )
		errx(1, "first insque: a.next != NULL");
	if ( a.prev != NULL )
		errx(1, "first insque: a.prev != NULL");
	struct object b;
	insque(&b, &a);
	if ( a.next != &b )
		errx(1, "second insque: a.next != &b");
	if ( a.prev != NULL )
		errx(1, "second insque: a.prev != NULL");
	if ( b.next != NULL )
		errx(1, "second insque: b.next != NULL");
	if ( b.prev != &a )
		errx(1, "second insque: b.prev != &a");
	struct object c;
	insque(&c, &a);
	if ( a.next != &c )
		errx(1, "third insque: a.next != &c");
	if ( a.prev != NULL )
		errx(1, "third insque: a.prev != NULL");
	if ( c.next != &b )
		errx(1, "third insque: c.next != &b");
	if ( c.prev != &a )
		errx(1, "third insque: c.prev != &a");
	if ( b.next != NULL )
		errx(1, "third insque: b.next != NULL");
	if ( b.prev != &c )
		errx(1, "third insque: b.prev != &c");
	return 0;
}
