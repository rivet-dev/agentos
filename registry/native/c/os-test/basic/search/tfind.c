/*[XSI]*/
/* Test whether a basic tfind invocation works. */

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
	// Search for foo.
	tnode* foo_search = tfind("foo", &root, compare);
	if ( !foo_search )
		errx(1, "tfind foo failed");
	if ( foo_search != foo_node )
		errx(1, "found wrong foo");
	// Search for bar.
	tnode* bar_search = tfind("bar", &root, compare);
	if ( !bar_search )
		errx(1, "tfind bar failed");
	if ( bar_search != bar_node )
		errx(1, "found wrong bar");
	// Search for qux.
	tnode* qux_search = tfind("qux", &root, compare);
	if ( qux_search )
		errx(1, "tfind found absent qux");
	return 0;
}
