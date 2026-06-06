#include <locale.h>
#ifdef newlocale
#undef newlocale
#endif
locale_t (*foo)(int, const char *, locale_t) = newlocale;
int main(void) { return 0; }
