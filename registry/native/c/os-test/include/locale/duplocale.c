#include <locale.h>
#ifdef duplocale
#undef duplocale
#endif
locale_t (*foo)(locale_t) = duplocale;
int main(void) { return 0; }
