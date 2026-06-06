#include <locale.h>
#ifdef setlocale
#undef setlocale
#endif
char *(*foo)(int, const char *) = setlocale;
int main(void) { return 0; }
