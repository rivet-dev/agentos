#include <ctype.h>
#ifdef islower_l
#undef islower_l
#endif
int (*foo)(int, locale_t) = islower_l;
int main(void) { return 0; }
