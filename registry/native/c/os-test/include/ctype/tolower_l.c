#include <ctype.h>
#ifdef tolower_l
#undef tolower_l
#endif
int (*foo)(int, locale_t) = tolower_l;
int main(void) { return 0; }
