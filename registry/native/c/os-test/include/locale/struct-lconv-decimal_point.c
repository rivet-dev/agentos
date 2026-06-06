#include <locale.h>
void foo(struct lconv* bar)
{
	char **qux = &bar->decimal_point;
	(void) qux;
}
int main(void) { return 0; }
