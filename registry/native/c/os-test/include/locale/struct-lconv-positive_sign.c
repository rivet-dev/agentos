#include <locale.h>
void foo(struct lconv* bar)
{
	char **qux = &bar->positive_sign;
	(void) qux;
}
int main(void) { return 0; }
