#include <locale.h>
void foo(struct lconv* bar)
{
	char **qux = &bar->mon_thousands_sep;
	(void) qux;
}
int main(void) { return 0; }
