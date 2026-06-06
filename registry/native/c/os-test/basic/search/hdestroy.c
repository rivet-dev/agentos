/*[XSI]*/
/* Test whether a basic hdestroy invocation works. */

#include <errno.h>
#include <search.h>

#include "../basic.h"

int main(void)
{
	if ( !hcreate(1024) )
		err(1, "hcreate");
	// Enter foo -> FOO.
	ENTRY foo = { .key = "foo", .data = "FOO" };
	ENTRY* foo_ptr = hsearch(foo, ENTER);
	if ( !foo_ptr )
		err(1, "hsearch ENTER foo");
	// POSIX does not say the key has to be allocated with malloc, and does not
	// say that hsearch obtains ownership of it, and it does not say that
	// hdestroy will free the key. In fact, the ownership of the key is
	// understated but tradition is that it belongs to the application and that
	// is what the standard implies. However, NetBSD introduced a bug in 2001
	// where it free'd the key in hdestroy. This bug propagated to OpenBSD,
	// FreeBSD, and Minix. From FreeBSD it went into DragonFly BSD and macOS. It
	// got fixed in NetBSD and FreeBSD in 2014, and the fix went into Minix in
	// 2016. However, DragonFly BSD, macOS, and OpenBSD still crash if the
	// key was not allocated by malloc. This behavior is a bug. The solution is
	// to assume the key is user allocated and stays alive until hdestroy, or to
	// strdup a copy internally.
	errno = 0;
	hdestroy();
	if ( errno )
		err(1, "hdestroy");
	return 0;
}
