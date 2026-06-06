/* Test whether a basic alphasort invocation works. */

#include <dirent.h>
#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	const struct dirent* a = malloc(sizeof(struct dirent) + sizeof("a"));
	const struct dirent* b = malloc(sizeof(struct dirent) + sizeof("b"));
	strcpy((char*) a->d_name, "a");
	strcpy((char*) b->d_name, "b");
	if ( !a || !b )
		err(1, "malloc");
	if ( !(alphasort(&a, &b) < 0)  )
		err(1, "alphasort: !(a < b)");
	if ( !(alphasort(&b, &a) > 0)  )
		err(1, "alphasort: !(b > a)");
	if ( !(alphasort(&a, &a) == 0)  )
		err(1, "alphasort: !(a == a)");
	return 0;
}
