#include <wchar.h>
#ifdef wcstod
#undef wcstod
#endif
double (*foo)(const wchar_t *restrict, wchar_t **restrict) = wcstod;
int main(void) { return 0; }
