/*[XSI]*/
/* Test whether a basic tdelete invocation works. */

#include <search.h>
#include <string.h>

#include "../basic.h"

typedef void tnode; // posix_tnode is not universal yet

static int compare(const void* a, const void* b)
{
	return strcmp((const char*) a, (const char*) b);
}

int main(void)
{
	// Insert foo and bar.
	tnode* root = NULL;
	tnode* foo_node = tsearch("foo", &root, compare);
	if ( !foo_node )
		errx(1, "tsearch foo failed");
	tnode* bar_node = tsearch("bar", &root, compare);
	if ( !bar_node )
		errx(1, "tsearch bar failed");
	// Delete qux.
	tnode* qux_delete = tdelete("qux", &root, compare);
	if ( qux_delete  )
		errx(1, "tdelete found absent qux");
	// Delete foo.
	tnode* foo_search = tfind("foo", &root, compare);
	if ( foo_search != foo_node  )
		errx(1, "tfind foo control failed");
	void* foo_delete = tdelete("foo", &root, compare);
	if ( !foo_delete )
		errx(1, "tdelete foo failed");
	foo_search = tfind("foo", &root, compare);
	if ( foo_search  )
		errx(1, "tdelete did not remove foo");
	// POSIX says that you're allowed to dereference the posix_tnode and get a
	// pointer to the element. That sort of implies a stability for these
	// posix_tnode objects. But how long does that apply? Is tsearch and tdelete
	// allowed to move keys between the posix_tnode objects? This happens on
	// FreeBSD and Haiku. Possibly allowing this could allow for more efficient
	// implementations, such as using an array of nodes instead of many small
	// malloc allocations, and allowing that array to shrink. It woud be nice
	// with a POSIX interpretation for the lifetime of posix_tnode* values.
	// Until then, I'm not going to consider this behavior as a bug that counts
	// against implementations.
#if 0
	if ( root != bar_node )
		errx(1, "root != bar_node (%s)", *(char**) root);
#endif
	// Delete bar.
	tnode* bar_search = tfind("bar", &root, compare);
	if ( bar_search == NULL  )
		errx(1, "tfind bar control failed");
#if 0
	if ( bar_search != bar_node  )
		errx(1, "tfind bar control failed");
#endif
	void* bar_delete = tdelete("bar", &root, compare);
	if ( !bar_delete )
		errx(1, "tdelete bar failed");
	bar_search = tfind("bar", &root, compare);
	if ( bar_search  )
		errx(1, "tdelete did not remove bar");
	if ( root != NULL )
		errx(1, "root != NULL");
	return 0;
}
