#include <wctype.h>
#ifdef wctype
#undef wctype
#endif
wctype_t (*foo)(const char *) = wctype;
int main(void) { return 0; }
