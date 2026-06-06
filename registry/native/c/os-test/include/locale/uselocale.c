#include <locale.h>
#ifdef uselocale
#undef uselocale
#endif
locale_t (*foo) (locale_t) = uselocale;
int main(void) { return 0; }
