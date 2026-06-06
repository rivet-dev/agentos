#include <wchar.h>
#ifdef wcsdup
#undef wcsdup
#endif
wchar_t *(*foo)(const wchar_t *) = wcsdup;
int main(void) { return 0; }
