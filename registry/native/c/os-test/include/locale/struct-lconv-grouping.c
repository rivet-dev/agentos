#include <locale.h>
void foo(struct lconv* bar)
{
	char **qux = &bar->grouping;
	(void) qux;
}
int main(void) { return 0; }
