#include <locale.h>
#ifdef getlocalename_l
#undef getlocalename_l
#endif
const char *(*foo)(int, locale_t) = getlocalename_l;
int main(void) { return 0; }
