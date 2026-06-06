#include <wctype.h>
#ifdef wctype_l
#undef wctype_l
#endif
wctype_t (*foo)(const char *, locale_t) = wctype_l;
int main(void) { return 0; }
