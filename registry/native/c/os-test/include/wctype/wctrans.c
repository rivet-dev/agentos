#include <wctype.h>
#ifdef wctrans
#undef wctrans
#endif
wctrans_t (*foo)(const char *) = wctrans;
int main(void) { return 0; }
