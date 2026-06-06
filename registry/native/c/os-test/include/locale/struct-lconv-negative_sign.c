#include <locale.h>
void foo(struct lconv* bar)
{
	char **qux = &bar->negative_sign;
	(void) qux;
}
int main(void) { return 0; }
