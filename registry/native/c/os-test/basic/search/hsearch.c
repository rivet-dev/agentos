/*[XSI]*/
/* Test whether a basic hsearch invocation works. */

#include <search.h>

#include "../basic.h"

int main(void)
{
	if ( !hcreate(1024) )
		err(1, "hcreate");
	// Enter foo -> FOO.
	ENTRY a = { .key = "foo", .data = "FOO" };
	ENTRY* a_ptr = hsearch(a, ENTER);
	if ( !a_ptr )
		err(1, "hsearch ENTER a");
	// Enter bar -> FOO.
	ENTRY b = { .key = "bar", .data = "BAR" };
	ENTRY* b_ptr = hsearch(b, ENTER);
	if ( !a_ptr )
		err(1, "hsearch ENTER b");
	if ( a_ptr == b_ptr )
		errx(1, "a_ptr == b_ptr");
	// Enter bar -> QUX (ignored because already present).
	ENTRY c = { .key = "foo", .data = "QUX" };
	ENTRY* c_ptr = hsearch(c, ENTER);
	if ( !c_ptr )
		err(1, "hsearch ENTER c");
	if ( c_ptr != a_ptr )
		errx(1, "c_ptr != a_ptr");
	if ( c_ptr != a_ptr )
		errx(1, "c_ptr == b_ptr");
	// Test looking up foo.
	ENTRY foo = { .key = "foo" };
	ENTRY* foo_ptr = hsearch(foo, FIND);
	if ( !foo_ptr )
		errx(1, "foo not found");
	if ( strcmp(foo_ptr->key, "foo") != 0 )
		errx(1, "foo had wrong key");
	if ( strcmp((char*) foo_ptr->data, "FOO") != 0 )
		errx(1, "foo did not contain FOO");
	if ( foo_ptr != a_ptr )
		errx(1, "foo_ptr != a_ptr");
	// Test looking up bar.
	ENTRY bar = { .key = "bar" };
	ENTRY* bar_ptr = hsearch(bar, FIND);
	if ( !bar_ptr )
		errx(1, "bar not found");
	if ( strcmp(bar_ptr->key, "bar") != 0 )
		errx(1, "bar had wrong key");
	if ( strcmp((char*) bar_ptr->data, "BAR") != 0 )
		errx(1, "bar did not contain BAR");
	if ( bar_ptr != b_ptr )
		errx(1, "bar_ptr != b_ptr");
	// Test looking up qux (absent).
	ENTRY qux = { .key = "qux" };
	ENTRY* qux_ptr = hsearch(qux, FIND);
	if ( qux_ptr )
		errx(1, "absent qux was found");
	return 0;
}
