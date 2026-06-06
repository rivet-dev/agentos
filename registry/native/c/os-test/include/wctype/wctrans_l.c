#include <wctype.h>
#ifdef wctrans_l
#undef wctrans_l
#endif
wctrans_t (*foo)(const char *, locale_t) = wctrans_l;
int main(void) { return 0; }
